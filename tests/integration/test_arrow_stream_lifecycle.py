import asyncio
import gc
import os
from pathlib import Path
import subprocess
import sys
import threading
import time

import pyarrow as pa
import pytest

from test_arrow_stream import ROWS
from test_json_lifecycle import LONG, MILLION, until


def test_arrow_async_cancel_active_snapshot_and_reuse(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            with pytest.raises(asyncio.CancelledError):
                async with pool.snapshot_async() as session:
                    async with session.stream_arrow_async(LONG, timeout=None) as stream:
                        active = asyncio.create_task(anext(stream))
                        await asyncio.sleep(0.05)
                        assert not active.done()
                        with pytest.raises(fb.PoolBusyError):
                            await anext(stream)
                        active.cancel()
                        await active
            assert await pool.fetch_val_async("SELECT 1 FROM RDB$DATABASE") == 1
            assert pool.stats()["native_cancels"] >= 1
    asyncio.run(run())


@pytest.mark.parametrize("foreign", [False, True])
def test_arrow_stream_idle_timeout_preserves_batch_and_error_context(fb, config, foreign):
    with fb.Pool(config(pool_size=1, stream_idle_timeout=0.05, prefetch_batches=0)) as pool:
        with pytest.raises(pa.ArrowException if foreign else fb.QueryTimeoutError) as caught:
            with pool.stream_arrow(ROWS, batch_rows=1, timeout=None) as stream:
                reader = pa.RecordBatchReader.from_stream(stream) if foreign else stream
                kept = next(reader)
                time.sleep(0.08)
                next(reader)
        if foreign:
            assert "QueryTimeout" in str(caught.value) and "stream_idle" in str(caught.value)
            reader.close()
        else:
            assert caught.value.phase == "stream_idle"
            kept = pa.record_batch(kept)
        assert kept[0][0].as_py() == 1
        assert pool.fetch_val("SELECT 2 FROM RDB$DATABASE") == 2


@pytest.mark.parametrize("close_owner", ["pool", "stream"])
def test_arrow_c_stream_close_during_foreign_fetch(fb, config, close_owner):
    pool = fb.Pool(config(pool_size=1, close_timeout=0.05 if close_owner == "pool" else 3))
    pool.warmup_sync()
    stream = pool.stream_arrow(LONG, timeout=0.4 if close_owner == "pool" else None).__enter__()
    reader = pa.RecordBatchReader.from_stream(stream)
    errors = []

    def consume():
        try:
            reader.read_next_batch()
        except BaseException as error:
            errors.append(error)

    thread = threading.Thread(target=consume)
    thread.start()
    try:
        time.sleep(0.08)
        assert thread.is_alive()
        if close_owner == "pool":
            with pytest.raises(fb.PoolTimeoutError):
                pool.close_sync()
            assert pool.stats()["leased"] + pool.stats()["cleaning"] > 0
        else:
            stream.close()
        thread.join(3)
        assert not thread.is_alive()
        assert len(errors) == 1 and isinstance(errors[0], pa.ArrowException)
        assert "fbfast" in str(errors[0])
        if close_owner == "pool":
            with pytest.raises(fb.QueryTimeoutError):
                stream.close()
        else:
            assert pool.fetch_val("SELECT 7 FROM RDB$DATABASE") == 7
    finally:
        pool.close_sync()
        thread.join(3)
        reader.close()
    assert pool.stats()["closed"] == 1 and pool.stats()["control_threads"] == 0


@pytest.mark.skipif(not hasattr(os, "fork"), reason="POSIX fork guard")
def test_arrow_exported_stream_fork_checks_pid_before_locks(fb, config):
    with fb.Pool(config(pool_size=1, prefetch_batches=0)) as pool:
        with pool.stream_arrow(ROWS, batch_rows=1) as stream:
            reader = pa.RecordBatchReader.from_stream(stream)
            child = os.fork()
            if child == 0:
                try:
                    try:
                        next(reader)
                    except pa.ArrowException as error:
                        assert "ForkSafety" in str(error)
                    else:
                        os._exit(2)
                    reader.close()
                    del reader, stream
                    gc.collect()
                    os._exit(0)
                except BaseException:
                    os._exit(3)
            _, status = os.waitpid(child, 0)
            assert os.waitstatus_to_exitcode(status) == 0
            assert next(reader)[0][0].as_py() == 1
            reader.close()


@pytest.mark.parametrize("mode", ["keyboard", "custom"])
def test_arrow_c_stream_honors_host_interrupt_handler(fb, mode):
    program = r'''
import os, signal, sys, threading, time
sys.path.insert(0, sys.argv[1])
import fbfast, pyarrow as pa
mode = sys.argv[2]
if mode == 'custom':
    def handler(signum, frame): raise RuntimeError('custom handler')
    signal.signal(signal.SIGINT, handler)
config = fbfast.FirebirdConfig(host='127.0.0.1', port=int(os.environ['FBFAST_TEST_PORT']),
    database='/var/lib/firebird/data/fbfast.fdb', user='FBFAST_READER',
    password=os.environ['FBFAST_TEST_PASSWORD'], role='FBFAST_READONLY', pool_size=1, min_idle=1, env_file=False)
sql = sys.argv[3]
with fbfast.Pool(config) as pool:
    start = time.monotonic()
    timer = threading.Timer(0.15, lambda: os.kill(os.getpid(), signal.SIGINT))
    timer.start()
    try:
        try:
            with pool.stream_arrow(sql, timeout=None) as stream: pa.table(stream)
        except (KeyboardInterrupt, pa.ArrowException, RuntimeError) as error:
            assert ('KeyboardInterrupt' if mode == 'keyboard' else 'custom handler') in str(type(error)) + str(error)
        else: raise AssertionError('interrupt was lost')
    finally: timer.join()
    assert time.monotonic() - start < 3
    assert pool.fetch_val('SELECT 17 FROM RDB$DATABASE') == 17
    assert pool.stats()['native_cancels'] >= 1
'''
    result = subprocess.run([sys.executable, "-E", "-s", "-c", program,
                             str(Path(fb.__file__).parent.parent), mode, LONG],
                            env=os.environ.copy(), capture_output=True, text=True, timeout=8)
    assert result.returncode == 0, result.stderr


def test_arrow_async_stream_million_rows_responsive_without_retaining_batches(fb, config, record_property):
    async def run():
        async with fb.Pool(config(pool_size=1, prefetch_batches=1, max_result_rows=1)) as pool:
            ticks, stop = [], False

            async def beat():
                while not stop:
                    ticks.append(time.perf_counter())
                    await asyncio.sleep(0.001)

            task = asyncio.create_task(beat())
            rows, kept = 0, None
            try:
                async with pool.stream_arrow_async(MILLION, batch_rows=2000, timeout=25) as stream:
                    async for batch in stream:
                        assert batch.num_rows <= 2000
                        rows += batch.num_rows
                        if kept is None:
                            kept = batch
            finally:
                stop = True
                await task
            await until(lambda: pool.stats()["leased"] == 0)
            assert rows == pool.stats()["rows"] == 1_000_000
            assert len(ticks) > 10
            gap = max(b - a for a, b in zip(ticks, ticks[1:]))
            assert gap < 0.15, gap
            record_property("arrow_stream_heartbeat_max_gap_ms", gap * 1000)
            record_property("build_id", fb.build_info()["build_id"])
        assert pa.record_batch(kept).num_rows == 2000
    asyncio.run(run())
