"""Real TCP disconnects through FastAPI/Starlette/Uvicorn and native row streams."""
import asyncio
import json
from contextlib import asynccontextmanager
from importlib.metadata import version
import socket
from types import SimpleNamespace

from fastapi import FastAPI
from starlette.responses import StreamingResponse
import uvicorn
import pytest


LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C"
ROWS = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT A.I AS A, B.I AS B FROM N A CROSS JOIN N B"
FIRST = {"rows": b"1\n", "ndjson": b'{"A":1,"B":1}\n', "array": b'[{"A":1,"B":1}'}


async def until(predicate):
    async with asyncio.timeout(3):
        while not predicate():
            await asyncio.sleep(0.005)


def application(fb, config, mode, transport):
    state = SimpleNamespace(name="http", pool=None, stream=None, body=None,
                            finished=asyncio.Event(), yielded=asyncio.Event(),
                            shutdown=asyncio.Event())

    @asynccontextmanager
    async def lifespan(app):
        state.pool = await fb.init_pool(config(pool_size=1, min_idle=0, prefetch_batches=0,
                                              query_timeout=None), name=state.name)
        try:
            yield
        finally:
            await fb.close_pool(state.name)
            state.shutdown.set()

    app = FastAPI(lifespan=lifespan)

    @app.get("/export")
    async def export():
        async def body():
            sql = LONG if mode == "fetch" else ROWS
            if mode == "complete":
                sql = "SELECT ID FROM FBFAST_TYPES ORDER BY ID"
            if transport == "rows":
                state.stream = fb.stream_async(sql, pool=state.name, batch_rows=1, timeout=None)
            else:
                state.stream = fb.stream_json_async(sql, pool=state.name, batch_rows=1,
                                                    timeout=None, format=transport)
            try:
                async with state.stream as batches:
                    async for batch in batches:
                        yield (str(batch[0][0]) + "\n").encode() if transport == "rows" else batch
                        state.yielded.set()
                        if mode == "error":
                            raise RuntimeError("Deliberate failure after HTTP headers")
                        if mode != "complete":
                            await asyncio.Event().wait()
            finally:
                state.finished.set()
        state.body = body()  # Keep alive: cleanup must not depend on GC.
        media = {"rows": "text/plain", "ndjson": "application/x-ndjson", "array": "application/json"}
        return StreamingResponse(state.body, media_type=media[transport])

    return app, state


@asynccontextmanager
async def serve(fb, config, mode, transport):
    app, state = application(fb, config, mode, transport)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
        server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port,
                                              log_level="critical", access_log=False,
                                              lifespan="on", timeout_graceful_shutdown=0.1))
        task = asyncio.create_task(server.serve(sockets=[listener]))
        try:
            await until(lambda: server.started or task.done())
            assert server.started and not task.done()
            yield port, state
        finally:
            server.should_exit = True
            await asyncio.wait_for(task, 5)
        assert state.shutdown.is_set()
        assert state.pool.stats()["closed"] == 1
        with pytest.raises(fb.PoolNotInitializedError):
            fb.get_pool(state.name)


async def request(port):
    reader, writer = await asyncio.open_connection("127.0.0.1", port)
    writer.write(b"GET /export HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    await writer.drain()
    headers = await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), 3)
    assert headers.startswith(b"HTTP/1.1 200 OK\r\n")
    assert b"transfer-encoding: chunked" in headers.lower()
    return reader, writer


async def chunk(reader):
    size = int((await asyncio.wait_for(reader.readline(), 3)).strip(), 16)
    body = await reader.readexactly(size + 2)
    assert body.endswith(b"\r\n")
    return body[:-2]


async def executing_on_server(fb, config):
    # A leased slot can still be preparing or checking cancellation locally.
    # Prove the expensive fetch is running before asserting a native cancel.
    async with fb.Pool(config(pool_size=1)) as observer:
        async with asyncio.timeout(3):
            while not await observer.fetch_val_async(
                "SELECT COUNT(*) FROM MON$STATEMENTS WHERE MON$STATE=1 "
                "AND MON$ATTACHMENT_ID<>CURRENT_CONNECTION "
                "AND MON$SQL_TEXT=CAST(? AS VARCHAR(1000))", [LONG]
            ):
                await asyncio.sleep(0.005)


@pytest.mark.parametrize("mode", ["idle", "fetch"])
@pytest.mark.parametrize("transport", ["rows", "ndjson", "array"])
def test_real_http_disconnect_cancels_stream_and_releases_slot(fb, config, mode, transport, record_property):
    record_property("http_versions", ", ".join(f"{name}={version(name)}" for name in
                                              ("fastapi", "starlette", "uvicorn", "anyio")))

    async def run():
        async with serve(fb, config, mode, transport) as (port, state):
            reader, writer = await request(port)
            try:
                if mode == "idle":
                    assert await chunk(reader) == FIRST[transport]
                    await asyncio.wait_for(state.yielded.wait(), 3)
                await until(lambda: state.pool.stats()["leased"] == 1)
                if mode == "fetch":
                    await executing_on_server(fb, config)
            finally:
                writer.close()
                await writer.wait_closed()
            await asyncio.wait_for(state.finished.wait(), 3)
            await until(lambda: state.pool.stats()["leased"] + state.pool.stats()["cleaning"] == 0)
            assert state.body is not None and state.stream is not None
            assert await fb.fetch_val_async("SELECT 9 FROM RDB$DATABASE", pool=state.name) == 9
            if mode == "fetch":
                assert state.pool.stats()["native_cancels"] >= 1
    asyncio.run(run())


@pytest.mark.parametrize("transport", ["rows", "ndjson", "array"])
def test_http_complete_stream_and_lifespan_cleanup(fb, config, transport):
    async def run():
        async with serve(fb, config, "complete", transport) as (port, state):
            reader, writer = await request(port)
            try:
                chunks = []
                while data := await chunk(reader):
                    chunks.append(data)
                encoded = b"".join(chunks)
                if transport == "rows":
                    assert encoded == b"1\n2\n"
                elif transport == "array":
                    assert json.loads(encoded) == [{"ID": 1}, {"ID": 2}]
                else:
                    assert [json.loads(line) for line in encoded.splitlines()] == [{"ID": 1}, {"ID": 2}]
            finally:
                writer.close()
                await writer.wait_closed()
            await asyncio.wait_for(state.finished.wait(), 3)
            assert state.pool.stats()["idle"] == 1
    asyncio.run(run())


@pytest.mark.parametrize("transport", ["rows", "ndjson", "array"])
def test_failure_after_headers_interrupts_body_without_false_success(fb, config, transport):
    async def run():
        async with serve(fb, config, "error", transport) as (port, state):
            reader, writer = await request(port)
            try:
                assert await chunk(reader) == FIRST[transport]
                # Connection ends without the terminal chunk; headers were already 200.
                assert await asyncio.wait_for(reader.read(), 3) == b""
            finally:
                writer.close()
                await writer.wait_closed()
            assert state.finished.is_set()
            assert await fb.fetch_val_async("SELECT 4 FROM RDB$DATABASE", pool=state.name) == 4
    asyncio.run(run())


@pytest.mark.parametrize("transport", ["rows", "ndjson", "array"])
def test_http_shutdown_cancels_active_body_before_pool_close(fb, config, transport):
    async def run():
        writer = None
        try:
            async with serve(fb, config, "idle", transport) as (port, state):
                reader, writer = await request(port)
                assert await chunk(reader) == FIRST[transport]
                await asyncio.wait_for(state.yielded.wait(), 3)
                assert state.pool.stats()["leased"] == 1
            assert state.finished.is_set()
            assert state.pool.stats()["closed"] == 1
        finally:
            if writer is not None:
                writer.close()
                await writer.wait_closed()
    asyncio.run(run())
