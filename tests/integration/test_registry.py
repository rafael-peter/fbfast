import asyncio
from concurrent.futures import ThreadPoolExecutor
from threading import Barrier, Event

import pytest


SQL = "SELECT ID FROM FBFAST_TYPES WHERE ID >= ? ORDER BY ID"


def test_missing_registry_never_opens_implicitly(fb, config):
    for name in ("default", "missing"):
        for operation in (
            lambda: fb.get_pool(name), lambda: fb.fetch_all(SQL, [1], pool=name),
            lambda: fb.fetch_one(SQL, [1], pool=name),
            lambda: fb.fetch_val(SQL, [1], pool=name),
            lambda: fb.explain(SQL, [1], pool=name),
            lambda: fb.stream(SQL, [1], pool=name),
            lambda: fb.stream_async(SQL, [1], pool=name),
        ):
            with pytest.raises(fb.PoolNotInitializedError) as failure:
                operation()
            assert isinstance(failure.value, fb.PoolError)
            assert failure.value.phase == "registry"
            assert failure.value.sqlstate is None
            assert failure.value.native_codes == failure.value.cleanup_errors == []

        async def run():
            for query in (fb.fetch_all_async, fb.fetch_one_async, fb.fetch_val_async,
                          fb.explain_async):
                with pytest.raises(fb.PoolNotInitializedError):
                    await query(SQL, [1], pool=name)
            await fb.close_pool(name)
        asyncio.run(run())
        fb.close_pool_sync(name)
    for name in ("", "  ", None, 2):
        with pytest.raises(fb.ConfigurationError):
            fb.init_pool_sync(config(), name=name)


def test_sync_module_delegates_share_registered_pool_and_limits(fb, config):
    pool = fb.init_pool_sync(config(pool_size=1, min_idle=0))
    other = None
    try:
        assert fb.get_pool() is pool and pool.stats()["connections_created"] == 1
        other = fb.init_pool_sync(config(pool_size=1, max_result_rows=1), name="bi")
        assert fb.get_pool("bi") is other
        with pytest.raises(fb.PoolAlreadyInitializedError):
            fb.init_pool_sync(config(), name="bi")
        assert fb.fetch_all(SQL, [1]) == [(1,), (2,)]
        assert fb.fetch_one(SQL, [2], as_dict=True) == {"ID": 2}
        assert fb.fetch_val(SQL, [2]) == 2
        assert 'Table "FBFAST_TYPES"' in fb.explain(SQL, [1])
        with fb.stream(SQL, [1], batch_rows=1, as_dict=True) as batches:
            assert list(batches) == [[{"ID": 1}], [{"ID": 2}]]
        with pytest.raises(fb.ResultLimitError):
            fb.fetch_all(SQL, [1], pool="bi")
        assert fb.fetch_val(SQL, [2], pool="bi") == 2
        assert pool.stats()["connections_created"] == 1
        assert pool.stats()["statement_hits"] >= 4
        fb.close_pool_sync("bi")
        assert fb.get_pool() is pool
        assert fb.fetch_val(SQL, [1]) == 1
    finally:
        fb.close_pool_sync("bi")
        fb.close_pool_sync()
    assert pool.stats()["closed"] == 1
    if other is not None:
        assert other.stats()["closed"] == 1
    with pytest.raises(fb.PoolNotInitializedError):
        fb.get_pool()
    fb.close_pool_sync()


def test_async_delegates_and_registry_cross_event_loops(fb, config):
    pool = asyncio.run(fb.init_pool(config(pool_size=1, min_idle=0)))

    async def query():
        assert fb.get_pool() is pool
        assert await fb.fetch_all_async(SQL, [1], as_dict=True) == [{"ID": 1}, {"ID": 2}]
        assert await fb.fetch_one_async(SQL, [2]) == (2,)
        assert await fb.fetch_val_async(SQL, [2]) == 2
        assert 'Table "FBFAST_TYPES"' in await fb.explain_async(SQL, [1])
        async with fb.stream_async(SQL, [1], batch_rows=1) as batches:
            assert [batch async for batch in batches] == [[(1,)], [(2,)]]
        with pytest.raises(fb.PoolAlreadyInitializedError):
            await fb.init_pool(config())
    try:
        asyncio.run(query())
        asyncio.run(query())
        assert pool.stats()["connections_created"] == 1
    finally:
        fb.close_pool_sync()
    pool = fb.init_pool_sync(config(pool_size=1))
    asyncio.run(fb.close_pool())
    asyncio.run(fb.close_pool())
    assert pool.stats()["closed"] == 1


def test_racing_initializers_have_exactly_one_winner(fb, config):
    barrier = Barrier(8)
    cfg = config(pool_size=1)

    def initialize(_):
        barrier.wait(timeout=5)
        try:
            return fb.init_pool_sync(cfg, name="race")
        except fb.PoolAlreadyInitializedError:
            return None
    try:
        with ThreadPoolExecutor(max_workers=8) as threads:
            outcomes = list(threads.map(initialize, range(8)))
        winners = [pool for pool in outcomes if pool is not None]
        assert len(winners) == 1
        assert fb.get_pool("race") is winners[0]
        assert winners[0].stats()["connections_created"] == 1
    finally:
        fb.close_pool_sync("race")


def test_close_during_open_cannot_publish_or_remove_replacement(fb, config, monkeypatch):
    opened, release = Event(), Event()
    original = fb.Pool.warmup_sync
    first = []

    def warmup(pool):
        original(pool)
        if not first:
            first.append(pool)
            opened.set()
            assert release.wait(5)
    monkeypatch.setattr(fb.Pool, "warmup_sync", warmup)
    with ThreadPoolExecutor(max_workers=1) as threads:
        pending = threads.submit(fb.init_pool_sync, config(pool_size=1), name="opening")
        try:
            assert opened.wait(3)
            with pytest.raises(fb.PoolNotInitializedError):
                fb.get_pool("opening")
            with pytest.raises(fb.PoolAlreadyInitializedError):
                asyncio.run(fb.init_pool(config(), name="opening"))
            fb.close_pool_sync("opening")
            assert first[0].stats()["closed"] == 1
            replacement = fb.init_pool_sync(config(pool_size=1), name="opening")
            release.set()
            with pytest.raises(fb.PoolClosedError):
                pending.result(timeout=3)
            assert fb.get_pool("opening") is replacement
            assert fb.fetch_val(SQL, [2], pool="opening") == 2
        finally:
            release.set()
            fb.close_pool_sync("opening")


@pytest.mark.parametrize("asynchronous", [False, True])
def test_failed_warmup_preserves_error_and_releases_name(fb, config, asynchronous):
    bad = config(role="NOT_GRANTED", pool_size=1)
    with pytest.raises(fb.RoleNotActiveError) as failure:
        if asynchronous:
            asyncio.run(fb.init_pool(bad, name="failure"))
        else:
            fb.init_pool_sync(bad, name="failure")
    assert failure.value.phase == "attach"
    with pytest.raises(fb.PoolNotInitializedError):
        fb.get_pool("failure")
    try:
        pool = fb.init_pool_sync(config(pool_size=1), name="failure")
        assert pool.fetch_val(SQL, [2]) == 2
    finally:
        fb.close_pool_sync("failure")
