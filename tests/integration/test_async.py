import asyncio
import decimal
import gc
from decimal import Decimal
import time

import pytest

LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C"
MILLION = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT A.I, B.I FROM N A CROSS JOIN N B"


async def until(predicate, timeout=3):
    end = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() >= end:
            raise AssertionError("Timed out waiting for pool state")
        await asyncio.sleep(0.005)


def test_async_same_results_and_successive_loops(fb, config):
    pool = fb.Pool(config(pool_size=2))

    async def run():
        await pool.warmup()
        assert await pool.fetch_val_async("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID=?", [1]) == Decimal("-12345678.9012")
        assert await pool.fetch_one_async("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [77]) is None
        assert await pool.fetch_all_async("SELECT ID FROM FBFAST_TYPES ORDER BY ID", as_dict=True) == [{"ID": 1}, {"ID": 2}]
        assert 'Table "FBFAST_TYPES"' in await pool.explain_async("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [1])
        with pytest.raises(fb.UnsupportedStatementError):
            await pool.fetch_val_async("DELETE FROM FBFAST_TYPES")
        values = await asyncio.gather(*(pool.fetch_val_async("SELECT CAST(? AS INTEGER) FROM RDB$DATABASE", [i]) for i in range(32)))
        assert values == list(range(32))

    try:
        asyncio.run(run())
        asyncio.run(run())
        assert pool.stats()["connections_created"] == 2
    finally:
        asyncio.run(pool.close())
    assert pool.stats()["closed"] == 2


def test_async_cancel_active_and_queued_preserves_capacity(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, max_waiters=1)) as pool:
            active = asyncio.create_task(pool.fetch_val_async(LONG, timeout=None))
            await until(lambda: pool.stats()["leased"] == 1)
            queued = asyncio.create_task(pool.fetch_val_async("SELECT 2 FROM RDB$DATABASE"))
            await until(lambda: pool.stats()["waiting"] == 1)
            with pytest.raises(fb.PoolBusyError):
                await pool.fetch_val_async("SELECT 3 FROM RDB$DATABASE")
            queued.cancel()
            with pytest.raises(asyncio.CancelledError):
                await queued
            await until(lambda: pool.stats()["waiting"] == 0)
            active.cancel()
            with pytest.raises(asyncio.CancelledError):
                await active
            await until(lambda: pool.stats()["leased"] + pool.stats()["cleaning"] == 0)
            assert await pool.fetch_val_async("SELECT 7 FROM RDB$DATABASE") == 7
            assert pool.stats()["native_cancels"] == 1
            assert pool.stats()["cancellations"] == 2
    asyncio.run(run())


def test_async_timeout_and_context_cleanup(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, query_timeout=0.05)) as pool:
            with pytest.raises(fb.QueryTimeoutError):
                await pool.fetch_val_async(LONG)
            unlimited = asyncio.create_task(pool.fetch_val_async(LONG, timeout=None))
            await asyncio.sleep(0.1)
            assert not unlimited.done()
            unlimited.cancel()
            with pytest.raises(asyncio.CancelledError):
                await unlimited
            assert await pool.fetch_val_async("SELECT 4 FROM RDB$DATABASE") == 4
        assert pool.stats()["closed"] == 1
    asyncio.run(run())


def test_async_cancel_before_submission_and_after_completion(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            before = pool.stats()["acquires"]
            request = asyncio.create_task(pool.fetch_val_async(LONG, timeout=None))
            request.cancel()
            with pytest.raises(asyncio.CancelledError):
                await request
            assert pool.stats()["acquires"] == before
            completed = asyncio.create_task(pool.fetch_val_async("SELECT 3 FROM RDB$DATABASE"))
            assert await completed == 3
            assert not completed.cancel()
            assert pool.stats()["cancellations"] == 0
    asyncio.run(run())


def test_async_cancel_during_python_materialization(fb, config, monkeypatch):
    calls = []

    class SlowDecimal(Decimal):
        def __new__(cls, *args):
            calls.append(1)
            time.sleep(0.002)
            return super().__new__(cls, *args)

    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            monkeypatch.setattr(decimal, "Decimal", SlowDecimal)
            request = asyncio.create_task(pool.fetch_all_async(
                "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT CAST(I AS DECIMAL(18,2)) FROM N", timeout=None))
            await until(lambda: len(calls) >= 5)
            assert pool.stats()["idle"] == 1
            request.cancel()
            with pytest.raises(asyncio.CancelledError):
                await request
            await until(lambda: pool.stats()["cancellations"] == 1)
            count = len(calls)
            await asyncio.sleep(0.02)
            assert len(calls) == count
            assert await pool.fetch_val_async("SELECT 6 FROM RDB$DATABASE") == 6
            assert pool.stats()["connections_created"] == 1
            assert pool.stats()["native_cancels"] == 0
    asyncio.run(run())


def test_async_deadline_includes_python_conversion(fb, config, monkeypatch):
    class SlowDecimal(Decimal):
        def __new__(cls, *args):
            time.sleep(0.1)
            return super().__new__(cls, *args)

    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            monkeypatch.setattr(decimal, "Decimal", SlowDecimal)
            with pytest.raises(fb.QueryTimeoutError) as error:
                await pool.fetch_val_async("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID=1", timeout=0.05)
            assert error.value.phase == "python_conversion"
            assert pool.stats()["timeouts"] == 1
            assert pool.stats()["idle"] == 1
    asyncio.run(run())


def test_async_heartbeat_during_million_tuple_materialization(fb, config, record_property):
    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            ticks = []
            collections = []
            collection_start = [0.0]
            stop = False

            def collection(phase, info):
                if phase == "start":
                    collection_start[0] = time.perf_counter()
                else:
                    collections.append((time.perf_counter() - collection_start[0], info["generation"]))

            async def heartbeat():
                while not stop:
                    ticks.append((time.perf_counter(), pool.stats()["idle"]))
                    await asyncio.sleep(0.002)

            beat = asyncio.create_task(heartbeat())
            gc.callbacks.append(collection)
            try:
                rows = await pool.fetch_all_async(MILLION, timeout=20)
                assert len(rows) == 1_000_000
                assert rows[0] == (1, 1) and rows[-1] == (1000, 1000)
            finally:
                gc.callbacks.remove(collection)
                stop = True
                await beat
            gaps = [b[0] - a[0] for a, b in zip(ticks, ticks[1:])]
            record_property("gc_max_pause_ms", max((duration for duration, _ in collections), default=0) * 1000)
            assert len(ticks) > 10
            assert max(gaps) < 0.15, {"max_gap": max(gaps), "longest_gc": sorted(collections, reverse=True)[:5]}
            # Native cleanup precedes Python conversion; idle heartbeats prove
            # responsiveness during materialization, not just while waiting on I/O.
            assert sum(idle for _, idle in ticks) >= 5
            record_property("build_id", fb.build_info()["build_id"])
            record_property("rows", len(rows))
            record_property("heartbeat_samples", len(ticks))
            record_property("heartbeat_max_gap_ms", max(gaps) * 1000)
            record_property("heartbeat_during_materialization", sum(idle for _, idle in ticks))
    asyncio.run(run())


def test_closed_loop_cancels_native_job_without_late_callback(fb, config):
    pool = fb.Pool(config(pool_size=1))
    pool.warmup_sync()
    loop = asyncio.new_event_loop()
    late = []
    original = loop.call_soon_threadsafe

    def record(*args, **kwargs):
        if loop.is_closed():
            late.append(True)
        return original(*args, **kwargs)

    loop.call_soon_threadsafe = record

    async def start():
        request = pool._core.value_async(LONG, timeout=None)
        await until(lambda: pool.stats()["leased"] == 1)
        return request

    request = loop.run_until_complete(start())
    loop.close()
    end = time.monotonic() + 3
    try:
        while pool.stats()["leased"] + pool.stats()["cleaning"]:
            assert time.monotonic() < end
            time.sleep(0.01)
        assert pool.stats()["native_cancels"] == 1
        assert pool.fetch_val("SELECT 9 FROM RDB$DATABASE") == 9
        assert not late
    finally:
        del request
        pool.close_sync()
