import asyncio
import time

import pytest

MEDIUM = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT CURRENT_CONNECTION, SUM(CAST(A.I AS BIGINT)*B.I) FROM N A CROSS JOIN N B GROUP BY CURRENT_CONNECTION"
LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C"


async def until(predicate):
    end = time.monotonic() + 3
    while not predicate():
        assert time.monotonic() < end
        await asyncio.sleep(0.005)


def test_clear_cache_is_lazy_and_rejects_closed_pool(fb, config):
    pool = fb.Pool(config(pool_size=2, min_idle=2))
    pool.clear_statement_cache_sync()
    asyncio.run(pool.clear_statement_cache())
    # A ready response does not bypass the delivery deadline.
    with pytest.raises(fb.QueryTimeoutError):
        pool.clear_statement_cache_sync(timeout=1e-9)
    with pytest.raises(fb.QueryTimeoutError):
        asyncio.run(pool.clear_statement_cache(timeout=1e-9))
    assert pool.stats()["connections_created"] == 0
    assert pool.stats()["closed"] == 2
    pool.close_sync()
    with pytest.raises(fb.PoolClosedError):
        pool.clear_statement_cache_sync()


def test_sync_and_async_clear_invalidate_every_attachment(fb, config):
    with fb.Pool(config(pool_size=2, min_idle=2)) as pool:
        async def both():
            values = await asyncio.gather(pool.fetch_one_async(MEDIUM), pool.fetch_one_async(MEDIUM))
            assert len({row[0] for row in values}) == 2
            assert {row[1] for row in values} == {250_500_250_000}

        asyncio.run(both())
        before = pool.stats()
        asyncio.run(both())
        assert pool.stats()["statement_hits"] == before["statement_hits"] + 2
        pool.clear_statement_cache_sync()
        before = pool.stats()
        asyncio.run(both())
        assert pool.stats()["statement_misses"] == before["statement_misses"] + 2
        assert pool.stats()["statement_hits"] == before["statement_hits"]
        asyncio.run(pool.clear_statement_cache())
        before = pool.stats()
        asyncio.run(both())
        assert pool.stats()["statement_misses"] == before["statement_misses"] + 2
        assert pool.stats()["connections_created"] == 2
        assert pool.stats()["cache_invalidations"] == 2


def test_cache_clear_timeout_does_not_interrupt_active_cursor(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, max_waiters=1)) as pool:
            active = asyncio.create_task(pool.fetch_val_async(LONG, timeout=None))
            await until(lambda: pool.stats()["leased"] == 1)
            pending = asyncio.create_task(pool.clear_statement_cache(timeout=0.1))
            await until(lambda: pool.stats()["waiting"] == 1)
            with pytest.raises(fb.PoolBusyError):
                await pool.clear_statement_cache()
            with pytest.raises(fb.QueryTimeoutError):
                await pending
            assert pool.stats()["leased"] == 1
            assert pool.stats()["native_cancels"] == 0
            assert pool.stats()["waiting"] == 0
            # Cancel only the explicit query request; the expired clear still
            # invalidates old cache entries once their owner can safely do so.
            active.cancel()
            with pytest.raises(asyncio.CancelledError):
                await active
            await pool.clear_statement_cache()
            assert await pool.fetch_val_async("SELECT 8 FROM RDB$DATABASE") == 8
            assert pool.stats()["waiting"] == 0
    asyncio.run(run())


def test_coalesced_clear_and_cancel_preserve_active_query_and_attachment(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, max_waiters=16)) as pool:
            connection = await pool.fetch_val_async("SELECT CURRENT_CONNECTION FROM RDB$DATABASE")
            active = asyncio.create_task(pool.fetch_one_async(MEDIUM))
            await until(lambda: pool.stats()["leased"] == 1)
            clears = [asyncio.create_task(pool.clear_statement_cache()) for _ in range(8)]
            await until(lambda: pool.stats()["waiting"] == 8)
            clears[0].cancel()
            with pytest.raises(asyncio.CancelledError):
                await clears[0]
            assert await active == (connection, 250_500_250_000)
            # No query is submitted to wake the owner after its active cursor.
            await asyncio.wait_for(asyncio.gather(*clears[1:]), timeout=3)
            assert pool.stats()["waiting"] == 0
            assert pool.stats()["native_cancels"] == 0
            assert pool.stats()["connections_created"] == 1
            assert await pool.fetch_val_async("SELECT CURRENT_CONNECTION FROM RDB$DATABASE") == connection
    asyncio.run(run())
