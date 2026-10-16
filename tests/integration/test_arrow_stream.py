import asyncio
import gc
import time

import polars as pl
import pyarrow as pa
import pytest


ROWS = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT I FROM N"
TYPES = "SELECT * FROM FBFAST_TYPES ORDER BY ID DESC"


@pytest.mark.parametrize("table", ["FBFAST_TYPES", "FBFAST_DECIMALS", "FBFAST_ZONED"])
def test_arrow_stream_all_native_types_equal_materialization(fb, config, table):
    sql = f"SELECT * FROM {table} ORDER BY ID"
    with fb.Pool(config(pool_size=1)) as pool:
        expected = pool.fetch_arrow(sql)
        with pool.stream_arrow(sql, batch_rows=2) as stream:
            schema = pa.schema(stream.schema)
            batch_result = pa.Table.from_batches([pa.record_batch(batch) for batch in stream], schema=schema)
        with pool.stream_arrow(sql, batch_rows=2) as stream:
            c_result = pa.table(stream)
    assert batch_result.equals(expected, check_metadata=True)
    assert c_result.equals(expected, check_metadata=True)


def test_arrow_batches_schema_capsules_nulls_lifetime_and_repeated_export(fb, config):
    with fb.Pool(config(pool_size=1, max_result_rows=1, prefetch_batches=0)) as pool:
        with pool.stream_arrow(TYPES, batch_rows=1) as stream:
            schema = pa.schema(stream.schema)
            assert schema.names == stream.schema.names
            assert pool.stats()["rows"] == 0
            batches = list(stream)
            assert all(isinstance(batch, fb.ArrowBatch) and len(batch) == batch.num_rows == 1 for batch in batches)
            assert all(batch.num_columns == len(schema) and batch.nbytes > 0 for batch in batches)
        with pool.stream_arrow("SELECT * FROM FBFAST_TYPES WHERE 1=0") as stream:
            assert pa.schema(stream.schema) == schema
            assert list(stream) == []
    gc.collect()
    left, right = pa.record_batch(batches[0]), pa.record_batch(batches[0])
    assert left.schema == right.schema == schema
    assert left.column(0).buffers()[1].address == right.column(0).buffers()[1].address
    with pytest.raises(NotImplementedError):
        batches[0].__arrow_c_array__(pa.schema([]).__arrow_c_schema__())
    for batch in batches:
        unused = batch.__arrow_c_array__()
        del unused
    capsules = batches[0].__arrow_c_array__()
    imported = pa.RecordBatch._import_from_c_capsule(*capsules)
    with pytest.raises((pa.ArrowInvalid, ValueError), match="released"):
        pa.RecordBatch._import_from_c_capsule(*capsules)
    del capsules
    assert imported[0][0].as_py() == 2
    table = pa.Table.from_batches([pa.record_batch(batch) for batch in batches])
    del batches, left, right
    gc.collect()
    assert table["ID"].to_pylist() == [2, 1]
    assert table["WIDE_INTEGER"].to_pylist() == [None, 2**127 - 1]


@pytest.mark.parametrize("prefetch", [0, 2])
def test_arrow_stream_demand_bound_and_early_context_close(fb, config, prefetch):
    with fb.Pool(config(pool_size=1, prefetch_batches=prefetch, max_result_rows=1)) as pool:
        with pool.stream_arrow(ROWS, batch_rows=4) as stream:
            time.sleep(0.04)
            assert pool.stats()["rows"] == 0
            kept = next(stream)
            assert kept.num_rows == 4
            time.sleep(0.08)
            assert 4 <= pool.stats()["rows"] <= 4 * (1 + prefetch)
        assert pool.stats()["leased"] == 0
        assert pool.fetch_val("SELECT COUNT(*) FROM MON$TRANSACTIONS WHERE MON$ATTACHMENT_ID=CURRENT_CONNECTION") == 1
    assert pa.record_batch(kept)[0].to_pylist() == [1, 2, 3, 4]


def test_arrow_c_stream_pull_exclusive_export_and_schema_before_fetch(fb, config):
    with fb.Pool(config(pool_size=1, prefetch_batches=0)) as pool:
        with pool.stream_arrow(ROWS, batch_rows=7) as stream:
            reader = pa.RecordBatchReader.from_stream(stream)
            assert reader.schema == pa.schema(stream.schema)
            assert pool.stats()["rows"] == 0
            with pytest.raises(fb.PoolBusyError):
                stream.__arrow_c_stream__()
            with pytest.raises(fb.PoolBusyError):
                next(stream)
            first = reader.read_next_batch()
            assert first.num_rows == 7
            assert pool.stats()["rows"] == 7
            rest = reader.read_all()
        assert pool.stats()["leased"] == 0
    reader.close()
    assert first[0].to_pylist() == list(range(1, 8))
    assert rest.num_rows == 993 and rest[0][-1].as_py() == 1000


@pytest.mark.parametrize("consumer", ["arrow", "polars"])
def test_arrow_c_stream_external_consumers_and_empty_schema(fb, config, consumer):
    with fb.Pool(config(pool_size=1)) as pool:
        with pool.stream_arrow("SELECT ID, LABEL FROM FBFAST_TYPES ORDER BY ID") as stream:
            result = pa.table(stream) if consumer == "arrow" else pl.from_arrow(stream, rechunk=False).struct.unnest()
        with pool.stream_arrow("SELECT ID, LABEL FROM FBFAST_TYPES WHERE 1=0") as stream:
            empty = pa.table(stream) if consumer == "arrow" else pl.from_arrow(stream, rechunk=False).struct.unnest()
        assert empty.schema == result.schema
        assert len(empty) == 0
    rows = result.to_pylist() if consumer == "arrow" else result.to_dicts()
    assert rows == [dict(ID=1, LABEL="Olá 日本語"), dict(ID=2, LABEL=None)]


@pytest.mark.parametrize("consume", [False, True])
def test_arrow_c_stream_early_release_snapshot_and_context_cleanup(fb, config, consume):
    with fb.Pool(config(pool_size=1, prefetch_batches=0)) as pool:
        with pool.snapshot() as session:
            with session.stream_arrow(ROWS, batch_rows=1) as stream:
                if consume:
                    reader = pa.RecordBatchReader.from_stream(stream)
                    kept = reader.read_next_batch()
                    reader.close()
                    del reader
                else:
                    capsule = stream.__arrow_c_stream__()
                    del capsule
                gc.collect()
            assert session.fetch_val("SELECT 42 FROM RDB$DATABASE") == 42
        assert pool.stats()["leased"] == 0
    if consume:
        assert kept[0][0].as_py() == 1


def test_arrow_empty_and_batches_async_snapshot_named_pool(fb, config):
    async def run():
        pool = await fb.init_pool(config(pool_size=1, max_result_rows=1), name="arrow-stream")
        try:
            async with fb.stream_arrow_async(ROWS, batch_rows=100, pool="arrow-stream") as stream:
                assert isinstance(stream, fb.AsyncArrowStream)
                assert stream.schema.names == ["I"]
                batches = [batch async for batch in stream]
            assert sum(batch.num_rows for batch in batches) == 1000
            async with pool.snapshot_async() as session:
                async with session.stream_arrow_async("SELECT ID FROM FBFAST_TYPES WHERE 1=0") as stream:
                    assert pa.schema(stream.schema).field(0).type == pa.int32()
                    assert [batch async for batch in stream] == []
                assert await session.fetch_val_async("SELECT 7 FROM RDB$DATABASE") == 7
        finally:
            await fb.close_pool("arrow-stream")
        assert pa.record_batch(batches[-1])[0][-1].as_py() == 1000
    asyncio.run(run())


def test_arrow_stream_aliases_limits_and_byte_bound(fb, config):
    with fb.Pool(config(pool_size=1, batch_bytes=32, max_result_rows=1)) as pool:
        with pytest.raises(fb.DataConversionError, match="Duplicate"):
            with pool.stream_arrow("SELECT ID, ID FROM FBFAST_TYPES"):
                pass
        with pytest.raises(fb.ConfigurationError):
            with pool.stream_arrow(ROWS, batch_rows=0):
                pass
        with pool.stream_arrow(ROWS, batch_rows=1000) as stream:
            assert sum(len(batch) for batch in stream) == 1000
        with pool.stream_arrow(TYPES) as stream:
            # A single row may exceed batch_bytes; it must never be split.
            batches = list(stream)
        assert [len(batch) for batch in batches] == [1, 1]
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
