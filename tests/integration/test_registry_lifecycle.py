import asyncio
import importlib
import os
from pathlib import Path
import socket
import subprocess
import sys
from threading import Event, Thread
import time

import pytest


def test_cancelled_initialization_drains_and_releases_name(fb, config, monkeypatch):
    original = fb.Pool.warmup
    pools = []

    async def run():
        warmed = asyncio.Event()

        async def warmup(pool):
            await original(pool)
            pools.append(pool)
            warmed.set()
            await asyncio.Event().wait()
        monkeypatch.setattr(fb.Pool, "warmup", warmup)
        pending = asyncio.create_task(fb.init_pool(config(pool_size=1), name="cancel-init"))
        try:
            await asyncio.wait_for(warmed.wait(), 3)
            with pytest.raises(fb.PoolNotInitializedError):
                fb.get_pool("cancel-init")
            pending.cancel()
            with pytest.raises(asyncio.CancelledError):
                await pending
            assert pools[0].stats()["closed"] == 1
            monkeypatch.setattr(fb.Pool, "warmup", original)
            replacement = await fb.init_pool(config(pool_size=1), name="cancel-init")
            assert fb.get_pool("cancel-init") is replacement
        finally:
            pending.cancel()
            await fb.close_pool("cancel-init")
    asyncio.run(run())


def test_cancelled_close_retains_name_until_retry(fb, config, monkeypatch):
    original = fb.Pool.close

    async def run():
        closing = asyncio.Event()

        async def close(pool):
            closing.set()
            await asyncio.Event().wait()
        pool = await fb.init_pool(config(pool_size=1), name="cancel-close")
        monkeypatch.setattr(fb.Pool, "close", close)
        pending = asyncio.create_task(fb.close_pool("cancel-close"))
        try:
            await asyncio.wait_for(closing.wait(), 3)
            assert pool.stats()["closing_pool"]
            pending.cancel()
            with pytest.raises(asyncio.CancelledError):
                await pending
            with pytest.raises(fb.PoolNotInitializedError):
                fb.get_pool("cancel-close")
            with pytest.raises(fb.PoolAlreadyInitializedError):
                fb.init_pool_sync(config(), name="cancel-close")
        finally:
            pending.cancel()
            monkeypatch.setattr(fb.Pool, "close", original)
            await fb.close_pool("cancel-close")
        assert pool.stats()["closed"] == 1
        replacement = await fb.init_pool(config(pool_size=1), name="cancel-close")
        try:
            assert replacement is not pool
        finally:
            await fb.close_pool("cancel-close")
    asyncio.run(run())


def test_delayed_close_and_stream_cannot_target_replacement(fb, config, monkeypatch):
    original = fb.Pool.close

    async def run():
        entered, release = asyncio.Event(), asyncio.Event()
        calls = []

        async def close(pool):
            calls.append(pool)
            if len(calls) == 1:
                entered.set()
                await release.wait()
            await original(pool)
        old = await fb.init_pool(config(pool_size=1), name="replace")
        retained = fb.stream_async("SELECT 1 FROM RDB$DATABASE", pool="replace")
        monkeypatch.setattr(fb.Pool, "close", close)
        delayed = asyncio.create_task(fb.close_pool("replace"))
        try:
            await asyncio.wait_for(entered.wait(), 3)
            await fb.close_pool("replace")
            replacement = await fb.init_pool(config(pool_size=1), name="replace")
            release.set()
            await delayed
            assert fb.get_pool("replace") is replacement
            assert old.stats()["closed"] == 1
            with pytest.raises(fb.PoolClosedError):
                async with retained:
                    pass
            assert await fb.fetch_val_async("SELECT 7 FROM RDB$DATABASE", pool="replace") == 7
        finally:
            release.set()
            await delayed
            monkeypatch.setattr(fb.Pool, "close", original)
            await fb.close_pool("replace")
    asyncio.run(run())


def test_repeated_cancel_during_native_attach_retains_cleanup_reservation(fb, config, monkeypatch):
    pools = []
    original = fb.Pool.__init__

    def capture(pool, cfg):
        original(pool, cfg)
        pools.append(pool)
    monkeypatch.setattr(fb.Pool, "__init__", capture)

    async def run():
        accepted, release, finished = asyncio.Event(), asyncio.Event(), asyncio.Event()

        async def peer(reader, writer):
            accepted.set()
            try:
                await asyncio.wait_for(release.wait(), 5)
            finally:
                writer.close()
                await writer.wait_closed()
                finished.set()

        server = await asyncio.start_server(peer, "127.0.0.1", 0)
        async with server:
            cfg = config(port=server.sockets[0].getsockname()[1], pool_size=1,
                         acquire_timeout=3, connect_timeout=3, close_timeout=2)
            pending = asyncio.create_task(fb.init_pool(cfg, name="cancel-attach"))
            try:
                await asyncio.wait_for(accepted.wait(), 3)
                assert pools[0].stats()["opening"] == 1
                pending.cancel()
                async with asyncio.timeout(1):
                    while not pools[0].stats()["closing_pool"]:
                        await asyncio.sleep(0.005)
                assert not pending.done()
                pending.cancel()
                with pytest.raises(asyncio.CancelledError) as failure:
                    await pending
                assert any("cleanup" in note for note in failure.value.__notes__)
                with pytest.raises(fb.PoolAlreadyInitializedError):
                    await fb.init_pool(config(), name="cancel-attach")
                with pytest.raises(fb.PoolNotInitializedError):
                    fb.get_pool("cancel-attach")
            finally:
                release.set()
                await asyncio.wait_for(finished.wait(), 3)
                await fb.close_pool("cancel-attach")
            assert pools[0].stats()["closed"] == 1
    asyncio.run(run())


def test_open_failure_and_close_timeout_keep_native_slot_reserved(fb, config, monkeypatch):
    accepted, release = Event(), Event()
    pools = []
    original = fb.Pool.__init__

    def capture(pool, cfg):
        original(pool, cfg)
        pools.append(pool)
    monkeypatch.setattr(fb.Pool, "__init__", capture)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(5)

        def peer():
            connection, _ = listener.accept()
            with connection:
                accepted.set()
                release.wait(5)
        server = Thread(target=peer)
        server.start()
        try:
            cfg = config(port=listener.getsockname()[1], pool_size=1,
                         acquire_timeout=0.05, close_timeout=0.03, connect_timeout=3)
            with pytest.raises(fb.PoolTimeoutError) as failure:
                fb.init_pool_sync(cfg, name="stalled")
            assert accepted.is_set()
            assert failure.value.phase == "acquire"
            assert failure.value.cleanup_errors
            assert isinstance(failure.value.cleanup_errors[0], fb.PoolTimeoutError)
            assert pools[0].stats()["opening"] == 1
            with pytest.raises(fb.PoolAlreadyInitializedError):
                fb.init_pool_sync(config(), name="stalled")
            with pytest.raises(fb.PoolNotInitializedError):
                fb.get_pool("stalled")
            with pytest.raises(fb.PoolTimeoutError):
                fb.close_pool_sync("stalled")
        finally:
            release.set()
            server.join(3)
            end = time.monotonic() + 3
            while pools[0].stats()["closed"] != 1:
                assert time.monotonic() < end
                time.sleep(0.005)
            fb.close_pool_sync("stalled")
        assert pools[0].stats()["closed"] == 1


@pytest.mark.skipif(not hasattr(os, "fork"), reason="POSIX fork guard")
def test_registry_fork_guard_precedes_locked_mutex(fb, config):
    pool = fb.init_pool_sync(config(pool_size=1), name="fork")
    registry = importlib.import_module("fbfast.registry")
    registry._lock.acquire()
    child = os.fork()
    if child == 0:
        try:
            for call in (lambda: fb.get_pool("fork"), lambda: fb.close_pool_sync("fork"),
                         lambda: fb.init_pool_sync(config(), name="new")):
                try:
                    call()
                except fb.ForkSafetyError:
                    pass
                else:
                    os._exit(2)
            os._exit(0)
        except BaseException:
            os._exit(3)
    registry._lock.release()
    try:
        end = time.monotonic() + 3
        while True:
            waited, status = os.waitpid(child, os.WNOHANG)
            if waited:
                break
            if time.monotonic() >= end:
                os.kill(child, 9)
                os.waitpid(child, 0)
                pytest.fail("Child touched inherited registry mutex")
            time.sleep(0.005)
        assert os.waitstatus_to_exitcode(status) == 0
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
    finally:
        fb.close_pool_sync("fork")


@pytest.mark.skipif(not hasattr(os, "fork"), reason="POSIX fork lifecycle")
def test_import_before_fork_allows_initialization_in_child(fb):
    # Fresh interpreter: import/config never start the native runtime in parent.
    script = r'''
import os, sys
sys.path.insert(0, sys.argv[1])
import fbfast
import fbfast.registry as registry
config = fbfast.FirebirdConfig(host="127.0.0.1", port=int(os.environ["FBFAST_TEST_PORT"]),
    database="/var/lib/firebird/data/fbfast.fdb", user="FBFAST_READER",
    password=os.environ["FBFAST_TEST_PASSWORD"], role="FBFAST_READONLY", env_file=False,
    pool_size=1, min_idle=0)
registry._lock.acquire()
child = os.fork()
if child == 0:
    try:
        pool = fbfast.init_pool_sync(config)
        assert fbfast.get_pool() is pool
        assert fbfast.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
        fbfast.close_pool_sync()
        os._exit(0)
    except BaseException:
        os._exit(2)
registry._lock.release()
_, status = os.waitpid(child, 0)
assert os.waitstatus_to_exitcode(status) == 0
try:
    fbfast.get_pool()
except fbfast.PoolNotInitializedError:
    pass
else:
    raise AssertionError("Child registry escaped process boundary")
'''
    result = subprocess.run([sys.executable, "-S", "-c", script,
                             str(Path(fb.__file__).parent.parent)],
                            capture_output=True, text=True, timeout=15)
    assert result.returncode == 0, result.stderr
