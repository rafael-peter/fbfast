import asyncio
import base64
from decimal import Decimal
import decimal
import json

import pytest


SCALARS = "SELECT ID, LABEL, PADDED, AMOUNT, WIDE_INTEGER, FLAG, DAY_VALUE, TIME_VALUE, STAMP_VALUE FROM FBFAST_TYPES ORDER BY ID"


def test_native_json_precision_types_escaping_and_owned_bytes(fb, config, monkeypatch):
    expected = {"ID": 1, "LABEL": "Olá 日本語", "PADDED": "a       ",
                "AMOUNT": "-12345678.9012", "WIDE_INTEGER": str(2**127 - 1), "FLAG": True,
                "DAY_VALUE": "2024-02-29", "TIME_VALUE": "12:34:56.123400",
                "STAMP_VALUE": "2024-02-29T12:34:56.123400"}

    class NoPythonDecimal(Decimal):
        def __new__(cls, *args):
            raise AssertionError("Native JSON must not materialize Decimal objects")

    with fb.Pool(config(pool_size=1)) as pool:
        before = pool.stats()
        monkeypatch.setattr(decimal, "Decimal", NoPythonDecimal)
        encoded = pool.fetch_json(SCALARS)
        assert isinstance(encoded, bytes)
        rows = json.loads(encoded)
        assert rows == [expected, {name: 2 if name == "ID" else None for name in expected}]
        after = pool.stats()
        assert after["rows"] - before["rows"] == 2
        assert after["result_bytes"] - before["result_bytes"] == len(encoded)
        text = 'quote " \\ slash /\n\r\t\b\f\x00 日本語 😀'
        assert json.loads(pool.fetch_json('SELECT CAST(? AS VARCHAR(100)) AS "a""b" FROM RDB$DATABASE', [text])) == [{'a"b': text}]
        assert b"\\n" in pool.fetch_json("SELECT CAST(? AS VARCHAR(10)) AS X FROM RDB$DATABASE", ["a\nb"])
    assert json.loads(encoded)[0] == expected


def test_json_integer_policy_depends_on_column_and_decimal_numbers_are_exact(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        sql = "SELECT CAST(ID AS SMALLINT) AS S, ID AS I, CAST(ID AS BIGINT) AS B, CAST(ID AS INT128) AS W FROM FBFAST_TYPES ORDER BY ID"
        assert json.loads(pool.fetch_json(sql)) == [dict(S=i, I=i, B=str(i), W=str(i)) for i in (1, 2)]
        assert json.loads(pool.fetch_json(sql, json_integers="number")) == [dict(S=i, I=i, B=i, W=i) for i in (1, 2)]
        for value in (-(2**127), 2**127 - 1):
            output = pool.fetch_json("SELECT CAST(? AS INT128) AS N FROM RDB$DATABASE", [value], json_integers="number")
            assert json.loads(output) == [{"N": value}]
        for value in (Decimal("-0.0001"), Decimal("0.0000"), Decimal("12345678.9012")):
            sql = "SELECT CAST(? AS NUMERIC(18,4)) AS N FROM RDB$DATABASE"
            assert json.loads(pool.fetch_json(sql, [value])) == [{"N": str(value)}]
            encoded = pool.fetch_json(sql, [value], json_decimals="number")
            assert json.loads(encoded, parse_float=Decimal, parse_int=Decimal) == [{"N": value}]


def test_json_binary_policies_empty_nulls_and_temporal_extremes(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        sql = "SELECT CAST(? AS VARCHAR(10) CHARACTER SET OCTETS) AS B FROM RDB$DATABASE"
        for value in (b"", b"\x00\xff\x80"):
            with pytest.raises(fb.DataConversionError, match="json_binary"):
                pool.fetch_json(sql, [value])
            assert json.loads(pool.fetch_json(sql, [value], json_binary="base64")) == [{"B": base64.b64encode(value).decode()}]
        assert json.loads(pool.fetch_json(sql, [None])) == [{"B": None}]
        output = pool.fetch_json("SELECT DATE '0001-01-01' AS A, DATE '9999-12-31' AS Z, TIME '23:59:59.9999' AS T FROM RDB$DATABASE")
        assert json.loads(output) == [{"A": "0001-01-01", "Z": "9999-12-31", "T": "23:59:59.999900"}]
        assert pool.fetch_json("SELECT ID FROM FBFAST_TYPES WHERE ID=0") == b"[]"


def test_json_aliases_orientation_and_materialized_row_limit(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        with pytest.raises(fb.DataConversionError) as failure:
            pool.fetch_json("SELECT 1/(ID-ID) AS DUP, ID AS DUP FROM FBFAST_TYPES")
        assert failure.value.phase == "describe"
        assert json.loads(pool.fetch_json("SELECT ID AS DUP, ID AS DUP FROM FBFAST_TYPES ORDER BY ID", orient="values")) == [[1, 1], [2, 2]]
    with fb.Pool(config(pool_size=1, max_result_rows=1)) as pool:
        with pytest.raises(fb.ResultLimitError):
            pool.fetch_json(SCALARS)
        assert pool.fetch_json("SELECT ID FROM FBFAST_TYPES WHERE ID=1") == b'[{"ID":1}]'


@pytest.mark.parametrize("value", [float("nan"), float("inf"), float("-inf")])
def test_json_non_finite_policy_with_real_float_binding(fb, config, value):
    with fb.Pool(config(pool_size=1)) as pool:
        sql = "SELECT CAST(? AS DOUBLE PRECISION) AS N FROM RDB$DATABASE"
        with pytest.raises(fb.DataConversionError, match="json_non_finite"):
            pool.fetch_json(sql, [value])
        assert pool.fetch_json(sql, [value], json_non_finite="null") == b'[{"N":null}]'
        with pool.stream_json(sql, [value], json_non_finite="null") as chunks:
            assert b"".join(chunks) == b'{"N":null}\n'


@pytest.mark.parametrize("name", ["orient", "json_integers", "json_decimals", "json_binary", "json_non_finite"])
def test_json_invalid_options_fail_before_native_open(fb, config, name):
    pool = fb.Pool(config())
    try:
        with pytest.raises(fb.ConfigurationError, match=name):
            pool.fetch_json("SELECT 1 FROM RDB$DATABASE", **{name: "invalid"})
        with pytest.raises(fb.ConfigurationError, match=name):
            with pool.stream_json("SELECT 1 FROM RDB$DATABASE", **{name: "invalid"}):
                pass
        assert pool.stats()["connections_created"] == 0
    finally:
        pool.close_sync()


@pytest.mark.parametrize("format", ["ndjson", "array"])
@pytest.mark.parametrize("batch_bytes", [1, 17, 4096])
def test_stream_json_bounds_stats_unicode_and_retained_chunks(fb, config, format, batch_bytes):
    sql = "SELECT ID, LABEL FROM FBFAST_TYPES ORDER BY ID"
    with fb.Pool(config(pool_size=1, batch_bytes=batch_bytes, max_result_rows=1, prefetch_batches=0)) as pool:
        before = pool.stats()
        stream = pool.stream_json(sql, format=format, batch_rows=2)
        assert isinstance(stream, fb.JsonStream)
        with stream as chunks:
            retained = list(chunks)
        after = pool.stats()
        assert after["rows"] - before["rows"] == 2
        assert after["result_bytes"] - before["result_bytes"] == sum(map(len, retained))
        if format == "array":
            assert all(len(chunk) <= batch_bytes for chunk in retained)
            rows = json.loads(b"".join(retained))
        else:
            assert all(chunk.endswith(b"\n") for chunk in retained)
            for chunk in retained:
                assert len(chunk) <= batch_bytes or len(chunk.splitlines()) == 1
            rows = [json.loads(line) for chunk in retained for line in chunk.splitlines()]
        assert rows == [{"ID": 1, "LABEL": "Olá 日本語"}, {"ID": 2, "LABEL": None}]
        with pool.stream_json("SELECT ID FROM FBFAST_TYPES WHERE ID=0", format=format) as chunks:
            assert b"".join(chunks) == (b"[]" if format == "array" else b"")
    assert all(isinstance(chunk, bytes) for chunk in retained)
    if format == "array":
        assert json.loads(b"".join(retained)) == rows
    else:
        assert [json.loads(line) for line in b"".join(retained).splitlines()] == rows


def test_named_json_delegates_async_and_snapshot_share_executor(fb, config):
    async def run():
        pool = await fb.init_pool(config(pool_size=1, prefetch_batches=0), name="json")
        try:
            assert fb.fetch_json("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [1], pool="json") == b'[{"ID":1}]'
            assert await fb.fetch_json_async("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [2], pool="json") == b'[{"ID":2}]'
            with fb.stream_json("SELECT ID FROM FBFAST_TYPES ORDER BY ID", pool="json") as chunks:
                assert b"".join(chunks) == b'{"ID":1}\n{"ID":2}\n'
            async with fb.stream_json_async("SELECT ID FROM FBFAST_TYPES ORDER BY ID", pool="json", format="array") as chunks:
                assert isinstance(chunks, fb.AsyncJsonStream)
                assert json.loads(b"".join([chunk async for chunk in chunks])) == [{"ID": 1}, {"ID": 2}]
            async with pool.snapshot_async() as session:
                tx = await session.fetch_val_async("SELECT CURRENT_TRANSACTION FROM RDB$DATABASE")
                sql = "SELECT CAST(CURRENT_TRANSACTION AS BIGINT) AS T FROM RDB$DATABASE"
                assert json.loads(await session.fetch_json_async(sql)) == [{"T": str(tx)}]
                async with session.stream_json_async(sql) as chunks:
                    assert [json.loads(chunk) async for chunk in chunks] == [{"T": str(tx)}]
                assert json.loads(session.fetch_json(sql)) == [{"T": str(tx)}]
            assert pool.stats()["connections_created"] == 1
        finally:
            await fb.close_pool("json")
    asyncio.run(run())
