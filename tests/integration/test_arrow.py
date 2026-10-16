from datetime import date, datetime, time, timedelta
from decimal import Decimal
import gc
import importlib

import polars as pl
import pyarrow as pa
import pytest

from test_decfloat import CASES as DECIMALS
from test_zoned import CASES as ZONES


TYPES = "SELECT * FROM FBFAST_TYPES ORDER BY ID DESC"


def test_arrow_schema_first_null_values_metadata_and_lifetime(fb, config):
    with fb.Pool(config(pool_size=1, fetch_size=1)) as pool:
        table = pool.fetch_arrow(TYPES)
        empty = pool.fetch_arrow("SELECT * FROM FBFAST_TYPES WHERE 1=0")
        assert pool.fetch_val("SELECT COUNT(*) FROM MON$TRANSACTIONS WHERE MON$ATTACHMENT_ID = CURRENT_CONNECTION") == 1
    gc.collect()
    assert isinstance(table, pa.Table)
    assert empty.num_rows == 0 and empty.schema == table.schema
    assert table["ID"].num_chunks == 2
    assert table["AMOUNT"].type == pa.decimal128(19, 4)
    assert table["WIDE_INTEGER"].type == pa.decimal256(39, 0)
    assert table["LABEL"].type == pa.large_string()
    assert table["BINARY_BLOB"].type == pa.large_binary()
    assert table["TIME_VALUE"].type == pa.time64("us")
    assert table["STAMP_VALUE"].type == pa.timestamp("us")
    assert table.schema.field("AMOUNT").metadata[b"fbfast.scale"] == b"-4"
    assert table.schema.field("WIDE_INTEGER").metadata[b"fbfast.sql_type"] == b"32752"
    assert table.to_pylist()[0] == {name: 2 if name == "ID" else None for name in table.column_names}
    assert table.to_pylist()[1] == dict(
        ID=1, LABEL="Olá 日本語", PADDED="a       ", AMOUNT=Decimal("-12345678.9012"),
        WIDE_INTEGER=Decimal(2**127 - 1), FLAG=True, DAY_VALUE=date(2024, 2, 29),
        TIME_VALUE=time(12, 34, 56, 123400), STAMP_VALUE=datetime(2024, 2, 29, 12, 34, 56, 123400),
        TEXT_BLOB="texto em BLOB", BINARY_BLOB=b"\0\xff\x80",
    )


@pytest.mark.parametrize("number", [-2**127, 2**127 - 1])
def test_arrow_int128_and_wide_numeric_never_round(fb, config, number):
    value = Decimal(f"{number}E-38")
    with fb.Pool(config(pool_size=1)) as pool:
        sql = 'SELECT CAST(? AS INT128) AS I, CAST(? AS NUMERIC(38,38)) AS D FROM RDB$DATABASE'
        table = pool.fetch_arrow(sql, [number, value])
        assert table["I"][0].as_py() == Decimal(number)
        assert table["D"].type == pa.decimal256(39, 38)
        assert table["D"][0].as_py().as_tuple() == value.as_tuple()
        with pytest.raises(fb.UnsupportedTypeError):
            pool.fetch_polars(sql, [number, value])
        frame = pool.fetch_polars(sql, [number, value], unsupported="string")
        assert frame["I"][0] == str(number)
        assert Decimal(frame["D"][0]).as_tuple() == value.as_tuple()


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_scalar_types_and_date_extremes(fb, config, consumer):
    sql = ("SELECT CAST(-32768 AS SMALLINT) AS S, CAST(-2147483648 AS INTEGER) AS I, "
           "CAST(-9223372036854775808 AS BIGINT) AS L, CAST(1.25 AS FLOAT) AS F, "
           "CAST(1.25 AS DOUBLE PRECISION) AS D, FALSE AS B, "
           "DATE '0001-01-01' AS LOW_DATE, DATE '9999-12-31' AS HIGH_DATE, "
           "TIMESTAMP '0001-01-01 00:00:00' AS LOW_STAMP, "
           "TIMESTAMP '9999-12-31 23:59:59.9999' AS HIGH_STAMP, "
           "CAST(x'00ff80' AS VARCHAR(3) CHARACTER SET OCTETS) AS RAW FROM RDB$DATABASE")
    with fb.Pool(config(pool_size=1)) as pool:
        result = getattr(pool, f"fetch_{consumer}")(sql)
    row = result.to_pylist()[0] if consumer == "arrow" else result.to_dicts()[0]
    assert row == dict(S=-32768, I=-2147483648, L=-9223372036854775808, F=1.25, D=1.25,
                      B=False, LOW_DATE=date.min, HIGH_DATE=date.max, LOW_STAMP=datetime.min,
                      HIGH_STAMP=datetime(9999, 12, 31, 23, 59, 59, 999900), RAW=b"\0\xff\x80")
    if consumer == "arrow":
        assert [result[name].type for name in ("S", "I", "L", "F", "D")] == [
            pa.int16(), pa.int32(), pa.int64(), pa.float32(), pa.float64()]
    else:
        assert [result.schema[name] for name in ("S", "I", "L", "F", "D")] == [
            pl.Int16, pl.Int32, pl.Int64, pl.Float32, pl.Float64]


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_decfloat_preserves_cohort_payload_and_specials(fb, config, consumer):
    with fb.Pool(config(pool_size=1, fetch_size=2)) as pool:
        result = getattr(pool, f"fetch_{consumer}")("SELECT D16, D34 FROM FBFAST_DECIMALS ORDER BY ID")
    rows = result.to_pylist() if consumer == "arrow" else result.to_dicts()
    for row, (_, left, right) in zip(rows, DECIMALS, strict=True):
        for actual, expected in zip(row.values(), (left, right), strict=True):
            assert actual is None if expected is None else Decimal(actual).as_tuple() == Decimal(expected).as_tuple()


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_timezone_structs_preserve_instants_zones_and_nulls(fb, config, monkeypatch, consumer):
    import fbfast.temporal as temporal

    def forbidden(*args):
        raise AssertionError("Analytical output must not construct a Python datetime per cell")

    monkeypatch.setattr(temporal, "_timestamp_from_native", forbidden)
    with fb.Pool(config(pool_size=1, fetch_size=3)) as pool:
        result = getattr(pool, f"fetch_{consumer}")("SELECT STAMP, CLOCK FROM FBFAST_ZONED ORDER BY ID")
        empty = getattr(pool, f"fetch_{consumer}")("SELECT STAMP, CLOCK FROM FBFAST_ZONED WHERE 1=0")
    assert result.schema == empty.schema
    rows = result.to_pylist() if consumer == "arrow" else result.to_dicts()
    for row, (stamp, zone, clock, clock_zone) in zip(rows, ZONES, strict=True):
        assert row["STAMP"] == (None if stamp is None else dict(utc=datetime.fromisoformat(stamp), zone=zone))
        assert row["CLOCK"] == (None if clock is None else dict(utc_time=time.fromisoformat(clock), zone=clock_zone))
    assert rows[1]["STAMP"]["utc"].utcoffset() == timedelta(0)


def test_polars_optional_consumer_empty_null_decimal_chunks_and_no_pyarrow(fb, config, monkeypatch):
    import fbfast.analytics as analytics
    original = importlib.import_module

    def guarded(name, *args, **kwargs):
        if name == "pyarrow" or name.startswith("pyarrow."):
            raise AssertionError("Polars must not import pyarrow")
        return original(name, *args, **kwargs)

    monkeypatch.setattr(analytics.importlib, "import_module", guarded)
    sql = "SELECT ID, AMOUNT, LABEL, FLAG FROM FBFAST_TYPES ORDER BY ID DESC"
    with fb.Pool(config(pool_size=1, fetch_size=1)) as pool:
        frame = pool.fetch_polars(sql)
        empty = pool.fetch_polars("SELECT ID, AMOUNT, LABEL, FLAG FROM FBFAST_TYPES WHERE 1=0")
        with pytest.raises(ValueError, match="unsupported"):
            pool.fetch_polars(sql, unsupported="ignore")
    assert frame.schema == empty.schema == {"ID": pl.Int32, "AMOUNT": pl.Decimal(19, 4), "LABEL": pl.String, "FLAG": pl.Boolean}
    assert frame["ID"].n_chunks() == 2
    assert frame.to_dicts() == [dict(ID=2, AMOUNT=None, LABEL=None, FLAG=None),
                              dict(ID=1, AMOUNT=Decimal("-12345678.9012"), LABEL="Olá 日本語", FLAG=True)]


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_limits_aliases_and_cleanup(fb, config, consumer):
    with fb.Pool(config(pool_size=1, fetch_size=1, max_result_rows=1)) as pool:
        fetch = getattr(pool, f"fetch_{consumer}")
        with pytest.raises(fb.ResultLimitError):
            fetch("SELECT ID FROM FBFAST_TYPES")
        with pytest.raises(fb.DataConversionError, match="Duplicate"):
            fetch("SELECT ID, ID FROM FBFAST_TYPES")
        result = fetch('SELECT 1 AS "á日" FROM RDB$DATABASE')
        assert (result.columns if consumer == "polars" else result.column_names) == ["á日"]
        assert pool.fetch_val("SELECT COUNT(*) FROM MON$TRANSACTIONS WHERE MON$ATTACHMENT_ID = CURRENT_CONNECTION") == 1
        assert pool.stats()["leased"] == 0


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_byte_limit_cleans_up_and_preserves_empty_schema(fb, config, consumer):
    with fb.Pool(config(pool_size=1, max_result_bytes=1)) as pool:
        fetch = getattr(pool, f"fetch_{consumer}")
        with pytest.raises(fb.ResultLimitError, match="max_result_bytes"):
            fetch("SELECT ID FROM FBFAST_TYPES")
        empty = fetch("SELECT ID FROM FBFAST_TYPES WHERE 1=0")
        assert len(empty) == 0
        assert pool.stats()["idle"] == 1


def test_arrow_capsules_share_buffers_and_outlive_exporter_and_pool(fb, config, monkeypatch):
    import fbfast.analytics as analytics
    monkeypatch.setattr(analytics, "_convert", lambda native, arrow: native)
    with fb.Pool(config(pool_size=1, fetch_size=1)) as pool:
        native = pool.fetch_arrow("SELECT ID, LABEL FROM FBFAST_TYPES ORDER BY ID")
    with pytest.raises(NotImplementedError, match="schema overrides"):
        native.__arrow_c_stream__(pa.schema([]).__arrow_c_schema__())
    for _ in range(10):
        # Unconsumed capsule destruction must release its reader safely.
        capsule = native.__arrow_c_stream__()
        del capsule
    capsule = native.__arrow_c_stream__()
    reader = pa.RecordBatchReader._import_from_c_capsule(capsule)
    with pytest.raises((pa.ArrowInvalid, ValueError), match="released"):
        pa.RecordBatchReader._import_from_c_capsule(capsule)
    retained = reader.read_next_batch()
    del reader, capsule
    assert retained.to_pylist() == [dict(ID=1, LABEL="Olá 日本語")]
    left, right = pa.table(native), pa.table(native)
    for a, b in zip(left["ID"].chunks, right["ID"].chunks, strict=True):
        assert a.buffers()[1].address == b.buffers()[1].address
    del native, right
    gc.collect()
    assert left.to_pylist() == [dict(ID=1, LABEL="Olá 日本語"), dict(ID=2, LABEL=None)]


def test_polars_numeric_buffers_are_shared_without_rechunk(fb, config, monkeypatch):
    import fbfast.analytics as analytics
    original = analytics._convert
    references = []

    def observe(native, arrow):
        references.append(pa.table(native))
        return original(native, arrow)

    monkeypatch.setattr(analytics, "_convert", observe)
    with fb.Pool(config(pool_size=1, fetch_size=1)) as pool:
        frame = pool.fetch_polars("SELECT ID FROM FBFAST_TYPES ORDER BY ID")
    arrays = [chunk.to_arrow() for chunk in frame["ID"].get_chunks()]
    assert len(arrays) == len(references[0]["ID"].chunks) == 2
    for imported, exported in zip(arrays, references[0]["ID"].chunks, strict=True):
        assert imported.buffers()[1].address == exported.buffers()[1].address
    references.clear()
    gc.collect()
    assert frame["ID"].to_list() == [1, 2]


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_blob_charsets_binary_and_size_limits(fb, config, consumer):
    sql = "SELECT TEXT_VALUE, BINARY_VALUE, NONE_VALUE, OCTETS_VALUE, LEGACY_VALUE FROM FBFAST_BLOBS WHERE ID=1"
    with fb.Pool(config(pool_size=1)) as pool:
        result = getattr(pool, f"fetch_{consumer}")(sql)
        row = result.to_pylist()[0] if consumer == "arrow" else result.to_dicts()[0]
        assert row == dict(TEXT_VALUE="Olá 日本語  ", BINARY_VALUE=b"\0\xff\x80\0",
                           NONE_VALUE="Olá".encode(), OCTETS_VALUE=b"\0\xff\x80\0", LEGACY_VALUE="€çã")
    with fb.Pool(config(pool_size=1, fallback_charset="UTF8", max_blob_bytes=32)) as pool:
        fetch = getattr(pool, f"fetch_{consumer}")
        result = fetch("SELECT NONE_VALUE FROM FBFAST_BLOBS WHERE ID=1")
        assert (result[0][0].as_py() if consumer == "arrow" else result[0, 0]) == "Olá"
        with pytest.raises(fb.DataConversionError):
            fetch("SELECT NONE_VALUE FROM FBFAST_BLOBS WHERE ID=2")
        with pytest.raises(fb.ResultLimitError):
            fetch("SELECT TEXT_VALUE FROM FBFAST_BLOBS WHERE ID=1023")
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1


@pytest.mark.parametrize("change", ["cast", "reorder"])
def test_polars_rejects_unverified_consumer_cast_or_reordering(fb, config, monkeypatch, change):
    original = pl.from_arrow

    def lossy(*args, **kwargs):
        assert kwargs["rechunk"] is False
        value = original(*args, **kwargs)
        frame = value.struct.unnest() if isinstance(value, pl.Series) else value
        return frame.cast({"I": pl.Float64}) if change == "cast" else frame.select("J", "I")

    monkeypatch.setattr(pl, "from_arrow", lossy)
    with fb.Pool(config(pool_size=1)) as pool:
        with pytest.raises(fb.DataConversionError, match="changed the native schema") as caught:
            pool.fetch_polars("SELECT 1 AS I, 2 AS J FROM RDB$DATABASE")
        assert caught.value.phase == "polars"
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_analytical_missing_extra_is_actionable_and_pool_reusable(fb, config, monkeypatch, consumer):
    import fbfast.analytics as analytics
    original = analytics.importlib.import_module

    def missing(name, *args, **kwargs):
        if name == ("pyarrow" if consumer == "arrow" else "polars"):
            raise ModuleNotFoundError(name)
        return original(name, *args, **kwargs)

    monkeypatch.setattr(analytics.importlib, "import_module", missing)
    with fb.Pool(config(pool_size=1)) as pool:
        with pytest.raises(ImportError, match=rf"fbfast\[{consumer}\]"):
            getattr(pool, f"fetch_{consumer}")("SELECT 1 AS I FROM RDB$DATABASE")
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
