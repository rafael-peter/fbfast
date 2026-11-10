"""A small report whose queries all see the same committed data.

``pool.snapshot()`` reserves one connection with a READ ONLY SNAPSHOT
transaction. It works with ``with`` and ``async with``; only one operation may
run at a time inside it, and its deadline (``snapshot_timeout``, 300 s by
default) also counts the time your code spends between queries.
Connection settings come from FB_* environment variables or .env.
"""
from __future__ import annotations

import asyncio

import fbfast

SUMMARY = """
SELECT
    SUM(CASE WHEN COALESCE(RDB$SYSTEM_FLAG, 0) = 0 THEN 1 ELSE 0 END) AS USER_TABLES,
    SUM(CASE WHEN COALESCE(RDB$SYSTEM_FLAG, 0) <> 0 THEN 1 ELSE 0 END) AS SYSTEM_TABLES
FROM RDB$RELATIONS
"""
COLUMNS = """
SELECT TRIM(RDB$RELATION_NAME) AS TABLE_NAME, COUNT(*) AS COLUMNS
FROM RDB$RELATION_FIELDS
GROUP BY 1
ORDER BY 2 DESC, 1
"""


def report(pool: fbfast.Pool) -> None:
    with pool.snapshot(timeout=60.0) as snap:
        summary = snap.fetch_one(SUMMARY, as_dict=True)
        print(f"tables: {summary}")
        # A stream inside a snapshot is the snapshot's single active operation:
        # finish (or close) it before running the next query.
        tables = batches_seen = 0
        with snap.stream(COLUMNS, batch_rows=20) as batches:
            for batch in batches:  # a list of tuples (as_dict=True gives dicts)
                tables += len(batch)
                batches_seen += 1
        print(f"streamed {tables} tables in {batches_seen} batches")
        top = snap.fetch_all(COLUMNS + " ROWS 3")
        print(f"widest: {top}")


async def report_async(pool: fbfast.Pool) -> None:
    async with pool.snapshot() as snap:
        tables = await snap.fetch_val_async("SELECT COUNT(*) FROM RDB$RELATIONS")
        fields = await snap.fetch_val_async("SELECT COUNT(*) FROM RDB$RELATION_FIELDS")
        print(f"async snapshot: {tables} tables, {fields} columns")


def main() -> None:
    config = fbfast.FirebirdConfig(min_idle=1)
    with fbfast.Pool(config) as pool:
        report(pool)
        asyncio.run(report_async(pool))


if __name__ == "__main__":
    main()
