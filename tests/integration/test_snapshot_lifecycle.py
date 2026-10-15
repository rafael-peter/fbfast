import asyncio
import decimal
from decimal import Decimal
import gc
import os
import time

import pytest

LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C"
DECIMALS = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT CAST(I AS DECIMAL(18,2)) FROM N"


async def until(predicate):
    end = time.monotonic() + 3
    while not predicate():
        assert time.monotonic() < end
        await asyncio.sleep(0.005)


@pytest.mark.skipif(not hasattr(os, "fork"), reason="POSIX fork guard")
def test_snapshot_fork_guards_operations_close_and_drop(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
        with pool.snapshot() as session:
            child = os.fork()
            if child == 0:
                try:
                    for call in (lambda: session.fetch_val("SELECT 1 FROM RDB$DATABASE"),
                                 lambda: session._native.value_async("SELECT 1 FROM RDB$DATABASE"),
                                 session._native.close_sync):
                        try:
                            call()
                        except fb.ForkSafetyError:
                            pass
                        else:
                            os._exit(2)
                    del session._native
                    gc.collect()
                    os._exit(0)
                except BaseException:
                    os._exit(3)
            _, status = os.waitpid(child, 0)
            assert os.waitstatus_to_exitcode(status) == 0
            assert session.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1


def test_snapshot_remains_exclusive_through_python_conversion(fb, config, monkeypatch):
    calls = []

    class SlowDecimal(Decimal):
        def __new__(cls, *args):
            calls.append(1)
            time.sleep(0.002)
            return super().__new__(cls, *args)

    async def run():
        async with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
            scope = pool.snapshot_async(timeout=None)
            session = await scope.__aenter__()
            monkeypatch.setattr(decimal, "Decimal", SlowDecimal)
            active = asyncio.create_task(session.fetch_all_async(DECIMALS, timeout=None))
            await until(lambda: len(calls) >= 5)
            with pytest.raises(fb.PoolBusyError):
                await session.fetch_val_async("SELECT 2 FROM RDB$DATABASE")
            active.cancel()
            with pytest.raises(asyncio.CancelledError):
                await active
            with pytest.raises(fb.FbFastError):
                await scope.close()
            assert pool.stats()["native_cancels"] == 0
            assert await pool.fetch_val_async("SELECT 3 FROM RDB$DATABASE") == 3
    asyncio.run(run())


def test_snapshot_deadline_includes_conversion_and_query_cannot_extend_scope(fb, config, monkeypatch):
    class SlowDecimal(Decimal):
        def __new__(cls, *args):
            time.sleep(0.1)
            return super().__new__(cls, *args)

    async def run():
        async with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
            monkeypatch.setattr(decimal, "Decimal", SlowDecimal)
            with pytest.raises(fb.QueryTimeoutError):
                async with pool.snapshot_async(timeout=0.05) as session:
                    await session.fetch_val_async("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID=1", timeout=None)
            assert await pool.fetch_val_async("SELECT 1 FROM RDB$DATABASE") == 1
    asyncio.run(run())


def test_snapshot_open_cancel_in_queue_does_not_cancel_other_lease(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
            active = asyncio.create_task(pool.fetch_val_async(LONG, timeout=None))
            await until(lambda: pool.stats()["leased"] == 1)
            scope = pool.snapshot_async()
            opening = asyncio.create_task(scope.__aenter__())
            await until(lambda: pool.stats()["waiting"] == 1)
            opening.cancel()
            with pytest.raises(asyncio.CancelledError):
                await opening
            assert pool.stats()["waiting"] == 0 and pool.stats()["native_cancels"] == 0
            active.cancel()
            with pytest.raises(asyncio.CancelledError):
                await active
            assert await pool.fetch_val_async("SELECT 1 FROM RDB$DATABASE") == 1
    asyncio.run(run())


def test_abandoned_native_snapshot_cancels_active_operation(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
            native = pool._core.snapshot(timeout=None)
            await native.warmup()
            active = native.value_async(LONG, timeout=None)
            await asyncio.sleep(0.04)
            del native
            gc.collect()
            with pytest.raises(fb.QueryCancelledError):
                await asyncio.wait_for(active, timeout=2)
            assert await pool.fetch_val_async("SELECT 2 FROM RDB$DATABASE") == 2
    asyncio.run(run())


def test_snapshot_open_preserves_connection_error(fb, config):
    pool = fb.Pool(config(role="FBFAST_MISSING_ROLE", min_idle=0))
    with pytest.raises(fb.RoleNotActiveError):
        with pool.snapshot():
            pass
    pool.close_sync()


@pytest.mark.parametrize("scope_timeout,query_timeout", [(0.05, None), (None, 0.05)])
def test_snapshot_native_deadline_counts_once_and_releases_capacity(fb, config, scope_timeout, query_timeout):
    with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
        with pytest.raises(fb.QueryTimeoutError):
            with pool.snapshot(timeout=scope_timeout) as session:
                session.fetch_val(LONG, timeout=query_timeout)
        assert pool.stats()["timeouts"] == 1
        assert pool.fetch_val("SELECT 7 FROM RDB$DATABASE") == 7


def test_cache_invalidation_waits_for_snapshot_lease_to_end(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
            async with pool.snapshot_async(timeout=None) as session:
                assert await session.fetch_val_async("SELECT 1 FROM RDB$DATABASE") == 1
                pending = asyncio.create_task(pool.clear_statement_cache(timeout=None))
                await until(lambda: pool.stats()["waiting"] == 1)
                assert not pending.done()
                assert await session.fetch_val_async("SELECT 2 FROM RDB$DATABASE") == 2
            await pending
            assert pool.stats()["cache_invalidations"] == 1
    asyncio.run(run())


def test_queued_pool_query_resumes_after_snapshot_and_child_generations(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
            async with pool.snapshot_async(timeout=None) as session:
                assert await session.fetch_val_async("SELECT 1 FROM RDB$DATABASE") == 1
                queued = asyncio.create_task(pool.fetch_val_async("SELECT 8 FROM RDB$DATABASE"))
                await until(lambda: pool.stats()["waiting"] == 1)
                assert not queued.done()
                assert await session.fetch_val_async("SELECT 2 FROM RDB$DATABASE") == 2
                assert await session.fetch_val_async("SELECT 3 FROM RDB$DATABASE") == 3
            assert await queued == 8
            assert pool.stats()["connections_created"] == 1
            assert pool.stats()["native_cancels"] == 0
    asyncio.run(run())


@pytest.mark.parametrize("close_stream", [True, False])
def test_abandoned_scope_releases_connection_even_when_child_stream_is_retained(fb, config, close_stream):
    with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
        scope = pool.snapshot(timeout=None)
        scope.__enter__()
        stream = scope.stream("SELECT ID FROM FBFAST_TYPES")
        stream.__enter__()
        if close_stream:
            stream.close()
        del scope
        gc.collect()
        end = time.monotonic() + 2
        while pool.stats()["leased"] + pool.stats()["cleaning"]:
            assert time.monotonic() < end
            time.sleep(0.005)
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
        if not close_stream:
            with pytest.raises(fb.FbFastError):
                stream.close()
