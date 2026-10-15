import asyncio
import json
import time

import pytest


LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) AS TOTAL FROM N A CROSS JOIN N B CROSS JOIN N C"
MILLION = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT A.I AS A, B.I AS B FROM N A CROSS JOIN N B"


async def until(predicate):
    async with asyncio.timeout(3):
        while not predicate():
            await asyncio.sleep(0.002)


def test_json_async_cancel_active_and_timeout_keep_pool_reusable(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, query_timeout=0.05)) as pool:
            with pytest.raises(fb.QueryTimeoutError):
                await pool.fetch_json_async(LONG)
            active = asyncio.create_task(pool.fetch_json_async(LONG, timeout=None))
            await until(lambda: pool.stats()["leased"] == 1)
            active.cancel()
            with pytest.raises(asyncio.CancelledError):
                await active
            await until(lambda: pool.stats()["leased"] + pool.stats()["cleaning"] == 0)
            assert await pool.fetch_json_async("SELECT 8 AS N FROM RDB$DATABASE") == b'[{"N":8}]'
            assert pool.stats()["native_cancels"] >= 1
    asyncio.run(run())


@pytest.mark.parametrize("format", ["ndjson", "array"])
def test_json_stream_timeout_does_not_complete_partial_document(fb, config, format):
    retained = []
    with fb.Pool(config(pool_size=1, prefetch_batches=0)) as pool:
        with pytest.raises(fb.QueryTimeoutError):
            with pool.stream_json(MILLION, format=format, batch_rows=1, timeout=0.1) as chunks:
                retained.append(next(chunks))
                time.sleep(0.15)
                retained.append(next(chunks))
        if format == "array":
            assert not b"".join(retained).endswith(b"]")
            with pytest.raises(json.JSONDecodeError):
                json.loads(b"".join(retained))
        else:
            assert [json.loads(line) for line in b"".join(retained).splitlines()] == [{"A": 1, "B": 1}]
        assert pool.fetch_json("SELECT 3 AS N FROM RDB$DATABASE") == b'[{"N":3}]'


def test_json_snapshot_stream_exclusivity_and_consumer_error_cleanup(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            async with pool.snapshot_async() as session:
                async with session.stream_json_async(MILLION, batch_rows=1) as chunks:
                    assert json.loads(await anext(chunks)) == {"A": 1, "B": 1}
                    with pytest.raises(fb.PoolBusyError):
                        await session.fetch_json_async("SELECT 1 FROM RDB$DATABASE")
                assert session.fetch_json("SELECT 2 AS N FROM RDB$DATABASE") == b'[{"N":2}]'
            with pytest.raises(RuntimeError, match="consumer stopped"):
                async with pool.stream_json_async(MILLION, batch_rows=1) as chunks:
                    assert await anext(chunks)
                    raise RuntimeError("consumer stopped")
            assert await pool.fetch_json_async("SELECT 4 AS N FROM RDB$DATABASE") == b'[{"N":4}]'
    asyncio.run(run())


def test_json_million_rows_responsive_without_python_cell_materialization(fb, config, record_property):
    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            ticks = []
            stop = False

            async def heartbeat():
                while not stop:
                    ticks.append(time.perf_counter())
                    await asyncio.sleep(0.002)
            beat = asyncio.create_task(heartbeat())
            try:
                output = await pool.fetch_json_async(MILLION, timeout=20)
            finally:
                stop = True
                await beat
            assert len(ticks) > 10
            gap = max(b - a for a, b in zip(ticks, ticks[1:]))
            assert gap < 0.15, gap
            assert output.startswith(b'[{"A":1,"B":1},')
            assert output.endswith(b'{"A":1000,"B":1000}]')
            assert pool.stats()["rows"] == 1_000_000
            assert pool.stats()["result_bytes"] == len(output)
            # Parse after timing to separate consumer-side Python allocations.
            assert len(json.loads(output)) == 1_000_000
            record_property("json_heartbeat_max_gap_ms", gap * 1000)
            record_property("json_output_bytes", len(output))
            record_property("build_id", fb.build_info()["build_id"])
    asyncio.run(run())
