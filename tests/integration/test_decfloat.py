import asyncio
import decimal
from decimal import Decimal, localcontext
import json

import pytest


CASES = [
    (1, "12345.67890012345", "12345678901234567890.12345678901234"),
    (2, "-0.0000", "-0E-33"), (3, "Infinity", "-Infinity"),
    (4, "NaN42", "-NaN24"), (5, "sNaN123", "-sNaN456"),
    (6, "1E-398", "1E-6176"),
    (7, "9.999999999999999E384", "9.999999999999999999999999999999999E6144"),
    (8, None, None), (9, "-1E-398", "-1E-6176"), (10, "0E+369", "0E+6111"),
]


def exact(value, text):
    if text is None:
        assert value is None
    else:
        assert isinstance(value, Decimal)
        assert value.as_tuple() == Decimal(text).as_tuple()


def test_decfloat_stored_values_preserve_context_precision_and_specials(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        with localcontext() as context:
            context.prec = 2
            context.Emax = 9
            context.Emin = -9
            context.traps[decimal.Inexact] = True
            context.traps[decimal.InvalidOperation] = True
            rows = pool.fetch_all("SELECT ID, D16, D34 FROM FBFAST_DECIMALS ORDER BY ID")
        for (id, left, right), expected in zip(rows, CASES, strict=True):
            assert id == expected[0]
            exact(left, expected[1])
            exact(right, expected[2])
        assert pool.fetch_all("SELECT D16, D34 FROM FBFAST_DECIMALS WHERE 1=0") == []
        with pool.snapshot() as session:
            with session.stream("SELECT ID, D16, D34 FROM FBFAST_DECIMALS ORDER BY ID", batch_rows=3) as batches:
                retained = list(batches)
    streamed = [row for batch in retained for row in batch]
    for (_, left, right), (_, a, b) in zip(streamed, CASES, strict=True):
        exact(left, a)
        exact(right, b)


@pytest.mark.parametrize("kind", [16, 34])
@pytest.mark.parametrize("text", ["1.2300", "-0.00", "0", "Infinity", "-Infinity", "NaN42", "-NaN42", "sNaN123", "-sNaN123", "1E-398"])
def test_decfloat_bind_roundtrip_specials_and_signed_zero(fb, config, kind, text):
    with fb.Pool(config(pool_size=1)) as pool:
        sql = f"SELECT CAST(? AS DECFLOAT({kind})) FROM RDB$DATABASE"
        with localcontext() as context:
            context.prec = 2
            value = pool.fetch_val(sql, [Decimal(text)])
        exact(value, text)
        assert pool.fetch_val(sql, [None]) is None
        assert pool.fetch_val(sql, [42]) == Decimal(42)


@pytest.mark.parametrize("kind, texts", [
    (16, ["1.2345678901234567", "1E+385", "1E-399", "NaN1234567890123456", "0E370", "-0E-399"]),
    (34, ["1.2345678901234567890123456789012345", "1E+6145", "1E-6177", "NaN1234567890123456789012345678901234", "0E6112", "-0E-6177"]),
])
def test_decfloat_bind_rejects_rounding_overflow_underflow_and_payload_loss(fb, config, kind, texts):
    with fb.Pool(config(pool_size=1)) as pool:
        sql = f"SELECT CAST(? AS DECFLOAT({kind})) FROM RDB$DATABASE"
        for text in texts:
            with pytest.raises(fb.DataConversionError) as caught:
                pool.fetch_val(sql, [Decimal(text)])
            assert caught.value.phase == "bind"
            assert "Parameter 1" in str(caught.value)
            assert text not in str(caught.value)
            assert not caught.value.cleanup_errors
        # Removing insignificant coefficient zeros may change cohort, never value.
        assert pool.fetch_val(sql, [Decimal("1." + "0" * 40)]) == Decimal(1)
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1


@pytest.mark.parametrize("kind, texts", [
    (16, ["9.999999999999999E384", "-1E-398", "0E369", "-0E-398", "NaN123456789012345"]),
    (34, ["9.999999999999999999999999999999999E6144", "-1E-6176", "0E6111", "-0E-6176", "NaN123456789012345678901234567890123"]),
])
def test_decfloat_bind_representable_domain_extremes(fb, config, kind, texts):
    with fb.Pool(config(pool_size=1)) as pool:
        for text in texts:
            exact(pool.fetch_val(f"SELECT CAST(? AS DECFLOAT({kind})) FROM RDB$DATABASE", [Decimal(text)]), text)


def test_decimal_bind_fixed_numeric_remains_exact_and_rejects_specials(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        sql = "SELECT CAST(? AS NUMERIC(18,4)) FROM RDB$DATABASE"
        for text in ["123.4567", "123.456700000000000000000000000000000000000000000000", "-0.0000"]:
            assert pool.fetch_val(sql, [Decimal(text)]) == Decimal(text)
        for text in ["NaN", "Infinity", "1.23456", "1E100000", "1E-100000"]:
            with pytest.raises(fb.DataConversionError):
                pool.fetch_val(sql, [Decimal(text)])


@pytest.mark.parametrize("format", ["ndjson", "array"])
def test_decfloat_json_is_native_exact_and_non_finite_requires_explicit_null(fb, config, monkeypatch, format):
    class NoDecimal(Decimal):
        def __new__(cls, *args, **kwargs):
            raise AssertionError("JSON must not instantiate Decimal objects")

    with fb.Pool(config(pool_size=1, batch_bytes=17)) as pool:
        monkeypatch.setattr(decimal, "Decimal", NoDecimal)
        sql = "SELECT ID, D16, D34 FROM FBFAST_DECIMALS ORDER BY ID"
        for mode in ["string", "number"]:
            with pytest.raises(fb.DataConversionError, match="Non-finite DECFLOAT"):
                pool.fetch_json(sql, json_decimals=mode)
            document = pool.fetch_json(sql, json_decimals=mode, json_non_finite="null")
            rows = json.loads(document, parse_float=Decimal)
            with pool.stream_json(sql, format=format, json_decimals=mode, json_non_finite="null") as chunks:
                data = b"".join(chunks)
            streamed = (json.loads(data, parse_float=Decimal) if format == "array"
                        else [json.loads(line, parse_float=Decimal) for line in data.splitlines()])
            assert streamed == rows
            for row, (_, left, right) in zip(rows, CASES, strict=True):
                for key, expected in [("D16", left), ("D34", right)]:
                    if expected is None or not Decimal(expected).is_finite():
                        assert row[key] is None
                    else:
                        exact(Decimal(row[key]), expected)
        assert pool.fetch_json("SELECT D16 FROM FBFAST_DECIMALS WHERE 1=0") == b"[]"


def test_decfloat_async_snapshot_and_stream(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            async with pool.snapshot_async() as session:
                exact(await session.fetch_val_async("SELECT D34 FROM FBFAST_DECIMALS WHERE ID=1"), CASES[0][2])
                async with session.stream_async("SELECT D16 FROM FBFAST_DECIMALS ORDER BY ID", batch_rows=2) as stream:
                    rows = [row async for batch in stream for row in batch]
                for row, (_, value, _) in zip(rows, CASES, strict=True):
                    exact(row[0], value)
                assert await session.fetch_json_async("SELECT D16 FROM FBFAST_DECIMALS WHERE ID=4", json_non_finite="null") == b'[{"D16":null}]'
    asyncio.run(run())
