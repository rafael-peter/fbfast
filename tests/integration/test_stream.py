import asyncio
import gc
import time

import pytest

MANY = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<103) SELECT I FROM N ORDER BY I"
MILLION = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT A.I, B.I FROM N A CROSS JOIN N B"
LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C"


def until(predicate):
    end = time.monotonic() + 3
    while not predicate():
        assert time.monotonic() < end
        time.sleep(0.005)


async def auntil(predicate, timeout=3):
    end = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < end
        await asyncio.sleep(0.005)


def test_stream_is_lazy_typed_batched_and_owns_results_after_close(fb, config):
    pool = fb.Pool(config(fetch_size=7))
    stream = pool.stream(MANY)
    assert pool.stats()["connections_created"] == 0
    with stream as batches:
        retained = list(batches)
    assert all(len(batch) == 7 for batch in retained[:-1])
    assert len(retained[-1]) == 5
    assert [row for batch in retained for row in batch] == [(i,) for i in range(1, 104)]
    with pool.stream("SELECT ID, LABEL, AMOUNT FROM FBFAST_TYPES WHERE ID = ?", [1], as_dict=True) as batches:
        rows = next(batches)
    assert rows == pool.fetch_all("SELECT ID, LABEL, AMOUNT FROM FBFAST_TYPES WHERE ID = 1", as_dict=True)
    with pool.stream("SELECT ID FROM FBFAST_TYPES WHERE 1=0") as batches:
        assert list(batches) == []
    with pytest.raises(RuntimeError):
        stream.__enter__()
    pool.close_sync()
    assert retained[0][0] == (1,)


@pytest.mark.parametrize("prefetch", [0, 1, 3])
def test_stream_prefetch_is_bounded_and_slot_stays_reserved(fb, config, prefetch):
    with fb.Pool(config(pool_size=1, prefetch_batches=prefetch, stream_idle_timeout=None)) as pool:
        baseline = pool.stats()["rows"]
        with pool.stream(MANY, batch_rows=10) as batches:
            time.sleep(0.05)
            assert pool.stats()["rows"] == baseline  # no demand, no fetch
            assert next(batches) == [(i,) for i in range(1, 11)]
            until(lambda: pool.stats()["rows"] == baseline + 10 * (prefetch + 1))
            time.sleep(0.05)
            assert pool.stats()["rows"] == baseline + 10 * (prefetch + 1)
            assert pool.stats()["leased"] == 1
        assert pool.stats()["leased"] == 0
        assert pool.stats()["native_cancels"] == 0
        assert pool.fetch_val("SELECT 8 FROM RDB$DATABASE") == 8


def test_stream_does_not_apply_total_materialization_limit(fb, config):
    with fb.Pool(config(max_result_rows=1, max_result_bytes=1)) as pool:
        with pytest.raises(fb.ResultLimitError):
            pool.fetch_all(MANY)
        with pool.stream(MANY, batch_rows=20) as batches:
            assert sum(map(len, batches)) == 103


def test_stream_validation_and_consumer_exception_preserve_cleanup(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        with pytest.raises(fb.ConfigurationError):
            with pool.stream(MANY, batch_rows=0):
                pass
        with pytest.raises(fb.DataConversionError):
            with pool.stream("SELECT ID AS X, ID AS X FROM FBFAST_TYPES", as_dict=True):
                pass
        with pytest.raises(ValueError, match="consumer"):
            with pool.stream(MANY) as batches:
                next(batches)
                raise ValueError("consumer")
        assert pool.fetch_val("SELECT 9 FROM RDB$DATABASE") == 9


@pytest.mark.parametrize("kind", ["idle", "query"])
def test_stream_deadlines_end_idle_cursors(fb, config, kind):
    with fb.Pool(config(pool_size=1, stream_idle_timeout=0.05 if kind == "idle" else None)) as pool:
        with pytest.raises(fb.QueryTimeoutError) as failure:
            with pool.stream(MANY, batch_rows=5, timeout=None if kind == "idle" else 0.1) as batches:
                next(batches)
                time.sleep(0.2)
                next(batches)
        if kind == "idle":
            assert failure.value.phase == "stream_idle"
        assert pool.fetch_val("SELECT 4 FROM RDB$DATABASE") == 4


def test_abandoned_stream_uses_worker_cleanup(fb, config):
    with fb.Pool(config(pool_size=1, stream_idle_timeout=None, query_timeout=None)) as pool:
        stream = pool.stream(MANY, batch_rows=5)
        stream.__enter__()
        next(stream)
        del stream
        gc.collect()
        until(lambda: pool.stats()["leased"] == 0 and pool.stats()["cleaning"] == 0)
        assert pool.fetch_val("SELECT 3 FROM RDB$DATABASE") == 3


def test_async_context_iteration_and_cache_clear_waits_for_stream(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            async with pool.stream_async(MANY, batch_rows=10, as_dict=True) as batches:
                assert await anext(batches) == [{"I": i} for i in range(1, 11)]
                clearing = asyncio.create_task(pool.clear_statement_cache())
                await auntil(lambda: pool.stats()["waiting"] == 1)
                assert not clearing.done()
            await clearing
            assert pool.stats()["native_cancels"] == 0
            async with pool.stream_async(MANY, batch_rows=30) as batches:
                retained = [row async for batch in batches for row in batch]
            assert retained == [(i,) for i in range(1, 104)]
            assert await pool.fetch_val_async("SELECT 2 FROM RDB$DATABASE") == 2
    asyncio.run(run())


def test_async_next_has_exclusive_consumer_and_cancellation_cleans_up(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, batch_bytes=64 * 1024 * 1024)) as pool:
            with pytest.raises(asyncio.CancelledError):
                async with pool.stream_async(MILLION, batch_rows=1_000_000, timeout=None) as batches:
                    active = asyncio.create_task(anext(batches))
                    await asyncio.sleep(0)
                    with pytest.raises(fb.PoolBusyError):
                        await anext(batches)
                    active.cancel()
                    await active
            assert await pool.fetch_val_async("SELECT 6 FROM RDB$DATABASE") == 6
    asyncio.run(run())


def test_async_cancel_during_queued_open_cleans_up(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            busy = asyncio.create_task(pool.fetch_val_async(LONG, timeout=None))
            await auntil(lambda: pool.stats()["leased"] == 1)
            stream = pool.stream_async(MANY, timeout=None)
            entering = asyncio.create_task(stream.__aenter__())
            await auntil(lambda: pool.stats()["waiting"] == 1)
            entering.cancel()
            with pytest.raises(asyncio.CancelledError):
                await entering
            assert pool.stats()["waiting"] == 0
            assert pool.stats()["native_cancels"] == 0
            busy.cancel()
            with pytest.raises(asyncio.CancelledError):
                await busy
            assert await pool.fetch_val_async("SELECT 5 FROM RDB$DATABASE") == 5
    asyncio.run(run())


def test_async_stream_materialization_keeps_consumer_exclusive_and_loop_responsive(fb, config, record_property):
    async def run():
        async with fb.Pool(config(pool_size=1, batch_bytes=128 * 1024 * 1024)) as pool:
            ticks = []
            running = True

            async def beat():
                while running:
                    ticks.append((time.monotonic(), pool.stats()["idle"] == 1))
                    await asyncio.sleep(0.001)

            heartbeat = asyncio.create_task(beat())
            try:
                async with pool.stream_async(MILLION, batch_rows=2_000_000, timeout=None) as batches:
                    active = asyncio.create_task(anext(batches))
                    try:
                        # Readiness bound, not a throughput target; latency is measured below.
                        await auntil(lambda: pool.stats()["rows"] == 1_000_000 and pool.stats()["idle"] == 1, timeout=10)
                        assert not active.done(), "must observe conversion after native cleanup"
                        with pytest.raises(fb.PoolBusyError):
                            await anext(batches)
                        rows = await active
                    finally:
                        if not active.done():
                            active.cancel()
                            await asyncio.gather(active, return_exceptions=True)
                    assert len(rows) == 1_000_000
                    with pytest.raises(StopAsyncIteration):
                        await anext(batches)
            finally:
                running = False
                await heartbeat
            gaps = [b[0] - a[0] for a, b in zip(ticks, ticks[1:])]
            assert len(ticks) > 10
            assert sum(idle for _, idle in ticks) > 5
            record_property("stream_heartbeat_max_gap_ms", max(gaps) * 1000)
            record_property("stream_heartbeat_during_materialization", sum(idle for _, idle in ticks))
            assert max(gaps) < 0.15
    asyncio.run(run())


def test_pool_close_drains_stream_waiting_for_demand(fb, config):
    pool = fb.Pool(config(pool_size=1, stream_idle_timeout=None, query_timeout=None))
    stream = pool.stream(MANY)
    stream.__enter__()
    pool.close_sync()
    assert pool.stats()["closed"] == 1
    with pytest.raises(fb.PoolClosedError):
        stream.close()
