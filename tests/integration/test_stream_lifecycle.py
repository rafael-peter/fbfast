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
def test_stream_fork_guard_and_child_drop_do_not_touch_parent_cursor(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        with pool.stream("SELECT ID FROM FBFAST_TYPES ORDER BY ID", batch_rows=1) as stream:
            child = os.fork()
            if child == 0:
                try:
                    for operation in (lambda: next(stream), stream.close, stream._native.next_async):
                        try:
                            operation()
                        except fb.ForkSafetyError:
                            pass
                        else:
                            os._exit(2)
                    del stream._native
                    gc.collect()
                    os._exit(0)
                except BaseException:
                    os._exit(3)
            _, status = os.waitpid(child, 0)
            assert os.waitstatus_to_exitcode(status) == 0
            assert next(stream) == [(1,)]
            assert next(stream) == [(2,)]


def test_stream_closed_loop_cancels_pending_fetch_without_late_callback(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        stream = pool._core.stream(LONG, timeout=None)
        stream.open_sync()
        loop = asyncio.new_event_loop()
        late = []
        original = loop.call_soon_threadsafe

        def record(*args, **kwargs):
            if loop.is_closed():
                late.append(True)
            return original(*args, **kwargs)

        loop.call_soon_threadsafe = record

        async def start():
            request = stream.next_async()
            await asyncio.sleep(0.05)
            assert not request.done()
            return request

        request = loop.run_until_complete(start())
        loop.close()
        end = time.monotonic() + 3
        while pool.stats()["leased"] + pool.stats()["cleaning"]:
            assert time.monotonic() < end
            time.sleep(0.01)
        with pytest.raises(fb.QueryCancelledError):
            stream.close_sync()
        assert pool.fetch_val("SELECT 9 FROM RDB$DATABASE") == 9
        assert not late
        del request


def test_stream_deadline_includes_python_conversion(fb, config, monkeypatch):
    class SlowDecimal(Decimal):
        def __new__(cls, *args):
            time.sleep(0.1)
            return super().__new__(cls, *args)

    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            monkeypatch.setattr(decimal, "Decimal", SlowDecimal)
            with pytest.raises(fb.QueryTimeoutError) as failure:
                async with pool.stream_async("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID=1", timeout=0.05) as stream:
                    await anext(stream)
            assert failure.value.phase == "python_conversion"
            assert pool.stats()["native_cancels"] == 0
            assert await pool.fetch_val_async("SELECT 7 FROM RDB$DATABASE") == 7
    asyncio.run(run())


def test_stream_cancel_during_conversion_stops_objects_without_cancelling_next_query(fb, config, monkeypatch):
    calls = []

    class SlowDecimal(Decimal):
        def __new__(cls, *args):
            calls.append(1)
            time.sleep(0.002)
            return super().__new__(cls, *args)

    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            monkeypatch.setattr(decimal, "Decimal", SlowDecimal)
            with pytest.raises(asyncio.CancelledError):
                async with pool.stream_async(DECIMALS, batch_rows=2000, timeout=None) as stream:
                    active = asyncio.create_task(anext(stream))
                    await until(lambda: len(calls) >= 5)
                    assert pool.stats()["idle"] == 1
                    active.cancel()
                    await active
            await until(lambda: pool.stats()["cancellations"] == 1)
            count = len(calls)
            await asyncio.sleep(0.03)
            assert len(calls) == count
            assert await pool.fetch_val_async("SELECT 6 FROM RDB$DATABASE") == 6
            assert pool.stats()["connections_created"] == 1
            assert pool.stats()["native_cancels"] == 0
    asyncio.run(run())
