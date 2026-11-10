"""Extract to Polars and Parquet: one DataFrame, or Arrow batches streamed to a file.

Requires: pip install "fbfast[polars,arrow]". Connection settings come from FB_*
environment variables or .env.

Firebird 4+ computes SUM over NUMERIC(18, x) or BIGINT as INT128, which Arrow
represents as Decimal256. Polars cannot hold Decimal256: CAST in SQL, or pass
``unsupported="string"`` to receive exact decimal text instead.
"""
from __future__ import annotations

import tempfile
from pathlib import Path

import polars as pl
import pyarrow as pa
import pyarrow.parquet as pq

import fbfast

SERIES = """
WITH RECURSIVE N(I) AS (
    SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I + 1 FROM N WHERE I < 100
),
ITEMS AS (
    SELECT CAST((A.I - 1) * 100 + B.I AS INTEGER) AS ID,
           CAST(A.I * B.I * 1.25 AS NUMERIC(18, 4)) AS AMOUNT,
           DATEADD(A.I DAY TO DATE '2026-01-01') AS DAY_VALUE,
           'item ' || B.I AS LABEL
    FROM N A CROSS JOIN N B
)
"""
ROWS = SERIES + "SELECT ID, AMOUNT, DAY_VALUE, LABEL FROM ITEMS"
TOTALS = SERIES + "SELECT DAY_VALUE, SUM(AMOUNT) AS TOTAL FROM ITEMS GROUP BY 1 ORDER BY 1 ROWS 3"
TOTALS_CAST = (SERIES + "SELECT DAY_VALUE, CAST(SUM(AMOUNT) AS NUMERIC(18, 4)) AS TOTAL "
               "FROM ITEMS GROUP BY 1 ORDER BY 1 ROWS 3")


def dataframe_to_parquet(pool: fbfast.Pool, directory: Path) -> None:
    frame = pool.fetch_polars(ROWS)  # no pyarrow needed for this call
    print(f"fetch_polars: {frame.shape} {dict(frame.schema)}")
    target = directory / "rows.parquet"
    frame.write_parquet(target)
    print(f"wrote {target.name}: {pl.read_parquet(target).height} rows")


def decimal256(pool: fbfast.Pool) -> None:
    try:
        pool.fetch_polars(TOTALS)
    except fbfast.UnsupportedTypeError as error:
        print(f"SUM(NUMERIC(18,4)) is INT128-based: {error}")
    text = pool.fetch_polars(TOTALS, unsupported="string")
    print(f"unsupported='string': {dict(text.schema)}")
    exact = pool.fetch_polars(TOTALS_CAST)
    print(f"CAST in SQL: {dict(exact.schema)}")


def stream_to_parquet(pool: fbfast.Pool, directory: Path) -> None:
    target = directory / "streamed.parquet"
    with pool.stream_arrow(ROWS, batch_rows=2_500) as stream:
        # The stream exports the Arrow C Stream interface; memory stays bounded.
        reader = pa.RecordBatchReader.from_stream(stream)
        with pq.ParquetWriter(target, reader.schema) as writer:
            for batch in reader:
                writer.write_batch(batch)
    print(f"stream_arrow wrote {pq.read_metadata(target).num_rows} rows to {target.name}")


def main() -> None:
    config = fbfast.FirebirdConfig(min_idle=1)
    with fbfast.Pool(config) as pool, tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary)
        dataframe_to_parquet(pool, directory)
        decimal256(pool)
        stream_to_parquet(pool, directory)


if __name__ == "__main__":
    main()
