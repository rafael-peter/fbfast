"""Native JSON: one array with fetch_json, or NDJSON chunks with stream_json.

JSON is written in Rust, without building Python rows. Streams hold one pooled
connection until the ``with`` block ends, so always use the context manager.
Connection settings come from FB_* environment variables or .env.
"""
from __future__ import annotations

import asyncio
import json
import tempfile
from collections.abc import AsyncIterator
from pathlib import Path

import fbfast

# 10,000 generated rows (a recursive CTE is limited to 1,024 levels, so cross join two).
ROWS = """
WITH RECURSIVE N(I) AS (
    SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I + 1 FROM N WHERE I < 100
)
SELECT CAST((A.I - 1) * 100 + B.I AS INTEGER) AS ID,
       CAST(A.I * B.I * 1.25 AS NUMERIC(18, 4)) AS AMOUNT,
       'item ' || B.I AS LABEL
FROM N A CROSS JOIN N B
"""


def whole_array(pool: fbfast.Pool) -> None:
    body = pool.fetch_json(ROWS + " ROWS 3")
    # Exact decimals are JSON strings by default (json_decimals="string").
    print(f"fetch_json: {body.decode()}")
    numbers = pool.fetch_json(ROWS + " ROWS 1", json_decimals="number")
    print(f"json_decimals='number': {numbers.decode()}")


def ndjson_to_file(pool: fbfast.Pool, target: Path) -> None:
    with target.open("wb") as output:
        with pool.stream_json(ROWS, format="ndjson", batch_rows=2_000) as chunks:
            for chunk in chunks:  # bytes; every chunk ends on a complete record
                output.write(chunk)
    with target.open("rb") as written:
        lines = written.read().splitlines()
    print(f"stream_json wrote {len(lines)} records; last={json.loads(lines[-1])}")


async def ndjson_body(pool: fbfast.Pool) -> AsyncIterator[bytes]:
    """What an HTTP framework would iterate (see docs/guide/recipes.md)."""
    async with pool.stream_json_async(ROWS + " ROWS 5") as chunks:
        async for chunk in chunks:
            yield chunk


async def async_stream(pool: fbfast.Pool) -> None:
    async for chunk in ndjson_body(pool):
        records = chunk.count(b"\n")
        print(f"async chunk with {records} records")


def main() -> None:
    config = fbfast.FirebirdConfig(min_idle=1)
    with fbfast.Pool(config) as pool, tempfile.TemporaryDirectory() as directory:
        whole_array(pool)
        ndjson_to_file(pool, Path(directory) / "rows.ndjson")
        asyncio.run(async_stream(pool))


if __name__ == "__main__":
    main()
