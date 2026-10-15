import asyncio
import gc
from pathlib import Path
import subprocess
import sys
import time

import pytest

LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C"
MANY = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<103) SELECT I FROM N"
VALUE = "SELECT VALUE_INTEGER FROM FBFAST_SNAPSHOT WHERE ID=1"
IDENTITY = "SELECT CURRENT_CONNECTION, CURRENT_TRANSACTION, MON$READ_ONLY, MON$ISOLATION_MODE FROM MON$TRANSACTIONS WHERE MON$TRANSACTION_ID=CURRENT_TRANSACTION"


def until(predicate):
    end = time.monotonic() + 3
    while not predicate():
        assert time.monotonic() < end
        time.sleep(0.005)


def mutate(value):
    script = Path(__file__).resolve().parents[2] / "scripts/fixture_mutation.py"
    subprocess.run([sys.executable, str(script), str(value)], check=True, capture_output=True)


def test_snapshot_lazy_identity_and_independent_pool_queries(fb, config):
    pool = fb.Pool(config())
    snapshot = pool.snapshot()
    assert pool.stats()["connections_created"] == 0
    with pytest.raises(RuntimeError):
        snapshot.fetch_val(VALUE)
    with snapshot as session:
        assert isinstance(session, fb.Snapshot)
        first = session.fetch_one(IDENTITY)
        assert first[2:] == (1, 1)
        assert session.fetch_one(IDENTITY) == first
        independent = pool.fetch_one(IDENTITY)
        assert independent[0] != first[0] and independent[2:] == (1, 4)
        assert session.fetch_all("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [1], as_dict=True) == [{"ID": 1}]
        assert session.explain("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [1])
    assert pool.stats()["leased"] == 0
    snapshot.close_sync()
    with pytest.raises(RuntimeError):
        snapshot.__enter__()
    pool.close_sync()


def test_snapshot_repeats_data_after_concurrent_commit_and_stream_shares_it(fb, config):
    mutate(10)
    try:
        with fb.Pool(config()) as pool:
            with pool.snapshot(timeout=5) as session:
                assert session.fetch_val(VALUE) == 10
                mutate(20)
                assert pool.fetch_val(VALUE) == 20
                assert session.fetch_val(VALUE) == 10
                with session.stream(VALUE, batch_rows=1) as batches:
                    assert next(batches) == [(10,)]
                    with pytest.raises(fb.PoolBusyError):
                        session.fetch_val(VALUE)
                assert session.fetch_val(VALUE) == 10
            assert pool.fetch_val(VALUE) == 20
    finally:
        mutate(10)


def test_snapshot_reserves_single_slot_until_cleanup_and_waiters_resume(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=0, acquire_timeout=0.03)) as pool:
        with pool.snapshot(timeout=None) as session:
            assert session.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
            with pytest.raises(fb.PoolTimeoutError):
                pool.fetch_val(VALUE)
            assert pool.stats()["leased"] == 1
        assert pool.fetch_val(VALUE) == 10
        assert pool.stats()["connections_created"] == 1


def test_snapshot_idle_deadline_closes_without_another_query(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
        scope = pool.snapshot(timeout=0.08)
        scope.__enter__()
        until(lambda: pool.stats()["leased"] == 0)
        with pytest.raises(fb.QueryTimeoutError):
            scope.fetch_val(VALUE)
        with pytest.raises(fb.QueryTimeoutError):
            scope.close_sync()
        assert pool.stats()["timeouts"] == 1
        assert pool.fetch_val(VALUE) == 10


def test_snapshot_sql_error_aborts_scope_without_implicit_transaction_restart(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
        with pytest.raises(fb.QueryError):
            with pool.snapshot() as scope:
                with pytest.raises(fb.QueryError):
                    scope.fetch_all("SELECT NONEXISTENT FROM FBFAST_TYPES")
                with pytest.raises(fb.FbFastError):
                    scope.fetch_val(VALUE)
        assert pool.fetch_val(VALUE) == 10


def test_abandoned_snapshot_cleans_up_without_io_in_destructor(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
        scope = pool.snapshot(timeout=None)
        scope.__enter__()
        scope.fetch_val(VALUE)
        del scope
        gc.collect()
        until(lambda: pool.stats()["leased"] == 0)
        assert pool.fetch_val(VALUE) == 10


def test_pool_close_drains_idle_snapshot(fb, config):
    pool = fb.Pool(config(pool_size=1, min_idle=0))
    scope = pool.snapshot(timeout=None)
    scope.__enter__()
    pool.close_sync()
    assert pool.stats()["closed"] == 1
    with pytest.raises(fb.PoolClosedError):
        scope.fetch_val(VALUE)


def test_async_snapshot_queries_streams_and_consumer_error(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
            async with pool.snapshot_async(timeout=None) as session:
                assert await session.fetch_val_async(VALUE) == 10
                assert (await session.fetch_one_async(IDENTITY))[2:] == (1, 1)
                assert await session.explain_async("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [1])
                async with session.stream_async(MANY, batch_rows=10) as batches:
                    assert sum([len(batch) async for batch in batches]) == 103
                assert await session.fetch_all_async("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [1]) == [(1,)]
            with pytest.raises(ValueError, match="consumer"):
                async with pool.snapshot_async():
                    raise ValueError("consumer")
            assert await pool.fetch_val_async(VALUE) == 10
    asyncio.run(run())


def test_async_snapshot_exclusive_query_cancellation_and_recovery(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, min_idle=0)) as pool:
            scope = pool.snapshot_async(timeout=None)
            session = await scope.__aenter__()
            active = asyncio.create_task(session.fetch_val_async(LONG, timeout=None))
            await asyncio.sleep(0.04)
            with pytest.raises(fb.PoolBusyError):
                await session.fetch_val_async(VALUE)
            active.cancel()
            with pytest.raises(asyncio.CancelledError):
                await active
            with pytest.raises(fb.FbFastError):
                await scope.close()
            assert await pool.fetch_val_async(VALUE) == 10
    asyncio.run(run())
