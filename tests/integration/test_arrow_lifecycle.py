import asyncio
import threading
import time

import pytest

from test_json_lifecycle import LONG, MILLION, until


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_sync_async_snapshot_named_pool_and_params(fb, config, consumer):
    sql = "SELECT CAST(? AS INTEGER) AS I FROM RDB$DATABASE"

    def value(result):
        return result["I"][0].as_py() if consumer == "arrow" else result["I"][0]

    with fb.Pool(config(pool_size=1)) as pool:
        with pool.snapshot() as session:
            assert value(getattr(session, f"fetch_{consumer}")(sql, [31])) == 31
    named = fb.init_pool_sync(config(pool_size=1), name="analytical")
    try:
        assert value(getattr(fb, f"fetch_{consumer}")(sql, [32], pool="analytical")) == 32
    finally:
        fb.close_pool_sync("analytical")
    assert named.stats()["closed"] == 1

    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            async with pool.snapshot_async() as session:
                assert value(await getattr(session, f"fetch_{consumer}_async")(sql, [33])) == 33
        await fb.init_pool(config(pool_size=1), name="analytical")
        try:
            assert value(await getattr(fb, f"fetch_{consumer}_async")(sql, [34], pool="analytical")) == 34
        finally:
            await fb.close_pool("analytical")
    asyncio.run(run())


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_async_timeout_and_active_cancel_cleanup(fb, config, consumer):
    async def run():
        async with fb.Pool(config(pool_size=1, query_timeout=0.05)) as pool:
            fetch = getattr(pool, f"fetch_{consumer}_async")
            options = {"unsupported": "string"} if consumer == "polars" else {}
            with pytest.raises(fb.QueryTimeoutError):
                await fetch(LONG, **options)
            active = asyncio.create_task(fetch(LONG, timeout=None, **options))
            await until(lambda: pool.stats()["leased"] == 1)
            active.cancel()
            with pytest.raises(asyncio.CancelledError):
                await active
            await until(lambda: pool.stats()["leased"] + pool.stats()["cleaning"] == 0)
            # Recovery is not a 50 ms cold-import benchmark for optional extras.
            result = await fetch("SELECT 9 AS I FROM RDB$DATABASE", timeout=2)
            assert len(result) == 1
    asyncio.run(run())


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
@pytest.mark.parametrize("finish", ["cancel", "timeout"])
def test_analytical_conversion_off_loop_keeps_snapshot_exclusive(fb, config, monkeypatch, consumer, finish):
    import fbfast.analytics as analytics
    original = analytics._convert
    entered, release = threading.Event(), threading.Event()
    threads = []

    def delayed(*args):
        threads.append(threading.get_ident())
        entered.set()
        assert release.wait(3)
        return original(*args)

    monkeypatch.setattr(analytics, "_convert", delayed)

    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            scope = pool.snapshot_async(timeout=None)
            session = await scope.__aenter__()
            fetch = getattr(session, f"fetch_{consumer}_async")
            task = asyncio.create_task(fetch("SELECT 1 AS I FROM RDB$DATABASE",
                                             timeout=0.15 if finish == "timeout" else None))
            try:
                await until(entered.is_set)
                assert threads == [threads[0]] and threads[0] != threading.get_ident()
                with pytest.raises(fb.PoolBusyError):
                    await session.fetch_val_async("SELECT 2 FROM RDB$DATABASE")
                if finish == "cancel":
                    task.cancel()
                    with pytest.raises(asyncio.CancelledError):
                        await task
                    with pytest.raises(fb.FbFastError):
                        await scope.close()
                else:
                    await asyncio.sleep(0.2)
            finally:
                release.set()
            if finish == "timeout":
                with pytest.raises(fb.QueryTimeoutError):
                    await task
                with pytest.raises(fb.FbFastError):
                    await scope.close()
            assert await pool.fetch_val_async("SELECT 3 FROM RDB$DATABASE") == 3
    asyncio.run(run())


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_million_rows_and_final_conversion_keep_loop_responsive(fb, config, monkeypatch, consumer, record_property):
    import fbfast.analytics as analytics
    original = analytics._convert
    interval, threads = [], []

    def observed(*args):
        threads.append(threading.get_ident())
        interval.append(time.perf_counter())
        try:
            return original(*args)
        finally:
            interval.append(time.perf_counter())

    monkeypatch.setattr(analytics, "_convert", observed)

    async def run():
        # 2,000 batches exercise final C Stream adaptation, not only SQL waiting.
        async with fb.Pool(config(pool_size=1, fetch_size=500)) as pool:
            ticks, stop = [], False

            async def heartbeat():
                while not stop:
                    ticks.append(time.perf_counter())
                    await asyncio.sleep(0.001)

            task = asyncio.create_task(heartbeat())
            try:
                result = await getattr(pool, f"fetch_{consumer}_async")(MILLION, timeout=25)
                await asyncio.sleep(0.002)
            finally:
                stop = True
                await task
            assert len(result) == 1_000_000
            assert threads == [threads[0]] and threads[0] != threading.get_ident()
            assert len(ticks) > 10
            gap = max(b - a for a, b in zip(ticks, ticks[1:]))
            conversion_ticks = sum(interval[0] <= tick <= interval[1] for tick in ticks)
            assert gap < 0.15, gap
            # A shorter-than-one-tick final adapter is also acceptable.
            assert conversion_ticks > 0 or interval[1] - interval[0] < 0.015
            assert pool.stats()["rows"] == 1_000_000
            record_property(f"{consumer}_heartbeat_max_gap_ms", gap * 1000)
            record_property(f"{consumer}_conversion_ms", (interval[1] - interval[0]) * 1000)
            record_property(f"{consumer}_conversion_ticks", conversion_ticks)
            record_property("build_id", fb.build_info()["build_id"])
    asyncio.run(run())
