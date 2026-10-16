import asyncio
import base64
import json
import time

import pytest


def test_blob_types_null_empty_and_lifetime(fb, config):
    with fb.Pool(config(pool_size=1, trim_char=True)) as pool:
        data = pool.fetch_all(
            "SELECT TEXT_VALUE, BINARY_VALUE, NONE_VALUE, OCTETS_VALUE, LEGACY_VALUE "
            "FROM FBFAST_BLOBS WHERE ID=1"
        )
        assert data == [("Olá 日本語  ", b"\x00\xff\x80\x00", "Olá".encode(),
                         b"\x00\xff\x80\x00", "€çã")]
        assert pool.fetch_one("SELECT TEXT_VALUE, BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=0") == ("", b"")
        assert pool.fetch_one("SELECT TEXT_VALUE, BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=3") == (None, None)
        assert pool.fetch_val("SELECT NONE_VALUE FROM FBFAST_BLOBS WHERE ID=2") == b"\xff\xfe\x80"
        assert pool.fetch_val("SELECT MON$READ_ONLY FROM MON$TRANSACTIONS WHERE MON$TRANSACTION_ID=CURRENT_TRANSACTION") == 1
    assert data[0][0] == "Olá 日本語  "  # Fully owned after cursor, transaction and pool close.


@pytest.mark.parametrize("size", [1023, 65534, 65535, 65536, 262145, 1048577])
def test_blob_sizes_exact_limits_and_reuse(fb, config, size):
    text = ("á日" * ((size + 1) // 2))[:size]
    sql = "SELECT TEXT_VALUE, BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=?"
    with fb.Pool(config(pool_size=1, max_blob_bytes=len(text.encode()))) as pool:
        assert pool.fetch_one(sql, [size]) == (text, b"x" * size)
        # Temporary expression BLOBs also remain valid across the cursor pump.
        assert pool.fetch_val("SELECT TEXT_VALUE || '!' FROM FBFAST_BLOBS WHERE ID=1") == "Olá 日本語  !"
    with fb.Pool(config(pool_size=1, max_blob_bytes=size - 1)) as pool:
        with pytest.raises(fb.ResultLimitError, match="max_blob_bytes") as caught:
            pool.fetch_val("SELECT BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=?", [size])
        assert caught.value.phase == "read_blob"
        assert not caught.value.cleanup_errors
        assert pool.fetch_val("SELECT 7 FROM RDB$DATABASE") == 7
        assert pool.stats()["idle"] == 1


def test_blob_none_fallback_invalid_decode_and_unknown_subtype(fb, config):
    with fb.Pool(config(pool_size=1, fallback_charset="UTF8")) as pool:
        assert pool.fetch_val("SELECT NONE_VALUE FROM FBFAST_BLOBS WHERE ID=1") == "Olá"
        assert pool.fetch_val("SELECT OCTETS_VALUE FROM FBFAST_BLOBS WHERE ID=1") == b"\x00\xff\x80\x00"
        with pytest.raises(fb.DataConversionError, match="UTF8 BLOB"):
            pool.fetch_val("SELECT NONE_VALUE FROM FBFAST_BLOBS WHERE ID=2")
        for suffix in ("WHERE ID=1", "WHERE 1=0"):
            with pytest.raises(fb.UnsupportedTypeError, match="subtype -7"):
                pool.fetch_all(f"SELECT UNKNOWN_VALUE FROM FBFAST_BLOBS {suffix}")
        assert pool.fetch_val("SELECT 3 FROM RDB$DATABASE") == 3


@pytest.mark.parametrize("format", ["ndjson", "array"])
def test_blobs_streams_json_snapshot_and_retained_results(fb, config, format):
    sql = "SELECT ID, TEXT_VALUE, BINARY_VALUE FROM FBFAST_BLOBS WHERE ID IN (0, 1, 3, 65535) ORDER BY ID"
    with fb.Pool(config(pool_size=1, batch_bytes=1024, prefetch_batches=0)) as pool:
        expected = pool.fetch_all(sql)
        with pool.snapshot() as session:
            with session.stream(sql, batch_rows=100) as stream:
                retained = list(stream)
            assert all(len(batch) == 1 for batch in retained)
            assert [row for batch in retained for row in batch] == expected
            with session.stream_json(sql, format=format, json_binary="base64") as chunks:
                encoded = list(chunks)
            document = b"".join(encoded)
            parsed = (json.loads(document) if format == "array"
                      else [json.loads(line) for line in document.splitlines()])
            assert parsed == [dict(ID=id, TEXT_VALUE=text, BINARY_VALUE=(
                base64.b64encode(binary).decode() if binary is not None else None
            )) for id, text, binary in expected]
            if format == "array":
                assert max(map(len, encoded)) <= 1024
            assert json.loads(session.fetch_json(sql, json_binary="base64")) == parsed
        with pytest.raises(fb.DataConversionError, match="binary"):
            pool.fetch_json(sql)
        assert pool.fetch_val("SELECT 9 FROM RDB$DATABASE") == 9
    assert retained[-1] == [expected[-1]]


def test_blob_async_and_stream_limit_cleanup(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, max_blob_bytes=1023)) as pool:
            assert await pool.fetch_val_async("SELECT TEXT_VALUE FROM FBFAST_BLOBS WHERE ID=1") == "Olá 日本語  "
            for kind in ("rows", "json"):
                factory = pool.stream_async if kind == "rows" else pool.stream_json_async
                with pytest.raises(fb.ResultLimitError, match="max_blob_bytes"):
                    async with factory("SELECT TEXT_VALUE FROM FBFAST_BLOBS WHERE ID=65535") as stream:
                        await anext(stream)
                assert await pool.fetch_val_async("SELECT 1 FROM RDB$DATABASE") == 1
            async with pool.snapshot_async() as session:
                assert await session.fetch_json_async("SELECT TEXT_VALUE FROM FBFAST_BLOBS WHERE ID=0") == b'[{"TEXT_VALUE":""}]'
    asyncio.run(run())


def test_blob_limit_is_per_cell_after_transliteration_and_bind_never_creates_blob(fb, config):
    with fb.Pool(config(pool_size=1, max_blob_bytes=4)) as pool:
        assert pool.fetch_one("SELECT BINARY_VALUE, OCTETS_VALUE FROM FBFAST_BLOBS WHERE ID=1") == (b"\x00\xff\x80\x00",) * 2
        with pytest.raises(fb.ResultLimitError, match="max_blob_bytes"):
            pool.fetch_val("SELECT LEGACY_VALUE FROM FBFAST_BLOBS WHERE ID=1")  # 3 stored WIN1252 bytes -> 7 UTF8 bytes
        sql = "SELECT CAST(? AS BLOB SUB_TYPE TEXT CHARACTER SET UTF8) FROM RDB$DATABASE"
        assert pool.fetch_val(sql, [None]) is None
        for parameter in ("private blob content", b"private blob content"):
            with pytest.raises(fb.DataConversionError) as caught:
                pool.fetch_val(sql, [parameter])
            assert caught.value.phase == "bind"
            assert "private blob content" not in str(caught.value)
        assert pool.fetch_val("SELECT 2 FROM RDB$DATABASE") == 2


def test_blob_also_counts_toward_materialized_result_budget(fb, config):
    with fb.Pool(config(pool_size=1, max_blob_bytes=2048, max_result_bytes=100)) as pool:
        with pytest.raises(fb.ResultLimitError, match="max_result_bytes"):
            pool.fetch_val("SELECT BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=1023")
        assert pool.fetch_all("SELECT ID FROM FBFAST_TYPES WHERE 1=0") == []
        assert pool.stats()["idle"] == 1


def test_blob_async_cancellation_deadline_and_heartbeat(fb, config, record_property):
    sql = "SELECT TEXT_VALUE, BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=1048577"

    async def ready(pool):
        async with asyncio.timeout(5):
            while pool.stats()["leased"] + pool.stats()["cleaning"]:
                await asyncio.sleep(0.002)

    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            with pytest.raises(fb.QueryTimeoutError):
                await pool.fetch_one_async(sql, timeout=0.02)
            await ready(pool)
            task = asyncio.create_task(pool.fetch_one_async(sql, timeout=None))
            async with asyncio.timeout(3):
                while not pool.stats()["leased"]:
                    await asyncio.sleep(0.001)
            await asyncio.sleep(0.02)
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
            await ready(pool)
            ticks = []
            finished = False

            async def heartbeat():
                while not finished:
                    ticks.append(time.perf_counter())
                    await asyncio.sleep(0.002)
            beat = asyncio.create_task(heartbeat())
            try:
                row = await pool.fetch_one_async(sql)
            finally:
                finished = True
                await beat
            assert len(ticks) > 5
            gap = max(b - a for a, b in zip(ticks, ticks[1:]))
            assert gap < 0.15, gap
            assert row == (("á日" * 524289)[:1048577], b"x" * 1048577)
            assert await pool.fetch_val_async("SELECT 5 FROM RDB$DATABASE") == 5
            record_property("blob_heartbeat_max_gap_ms", gap * 1000)
            record_property("build_id", fb.build_info()["build_id"])
    asyncio.run(run())
