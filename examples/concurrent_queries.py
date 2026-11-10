"""Run several independent queries at once with run_all / run_many.

Each ``fbfast.Query`` describes one query and its result type. A group runs at
most ``concurrency`` queries at a time (default and maximum: ``pool_size``), so
it never overflows the pool queue the way a large ``asyncio.gather`` can.
Connection settings come from FB_* environment variables or .env.
"""
from __future__ import annotations

import asyncio
import time

import fbfast
from fbfast import Query

COUNT_BY_FLAG = "SELECT COUNT(*) FROM RDB$RELATIONS WHERE COALESCE(RDB$SYSTEM_FLAG, 0) = ?"
NAMES = """
SELECT TRIM(RDB$RELATION_NAME) AS NAME
FROM RDB$RELATIONS
WHERE RDB$RELATION_ID < ?
ORDER BY RDB$RELATION_ID
"""


def run_all_example(pool: fbfast.Pool) -> None:
    user_tables = Query.fetch_val(COUNT_BY_FLAG, [0])
    system_tables = Query.fetch_val(COUNT_BY_FLAG, [1])
    names = Query.fetch_all(NAMES, [4], as_dict=True)  # results[names]: list[dict]
    as_json = Query.fetch_json("SELECT CURRENT_DATE AS TODAY FROM RDB$DATABASE")

    started = time.perf_counter()
    results = pool.run_all([user_tables, system_tables, names, as_json])
    elapsed = time.perf_counter() - started
    print(f"run_all finished {len(results)} queries in {elapsed * 1000:.1f} ms")
    print(f"  user tables={results[user_tables]} system tables={results[system_tables]}")
    print(f"  names={[row['NAME'] for row in results[names]]}")
    print(f"  json={results[as_json].decode()}")


def fail_fast_off_example(pool: fbfast.Pool) -> None:
    good = Query.fetch_val("SELECT 1 FROM RDB$DATABASE")
    bad = Query.fetch_val("SELECT NO_SUCH_COLUMN FROM RDB$DATABASE")
    # fail_fast=False runs everything; a failed query raises only when looked up.
    results = pool.run_all([good, bad], fail_fast=False)
    print(f"  good={results[good]} errors={[type(e).__name__ for e in results.errors.values()]}")


def run_many_example(pool: fbfast.Pool) -> None:
    queries = [Query.fetch_val("SELECT CAST(? AS INTEGER) * 10 FROM RDB$DATABASE", [n])
               for n in range(1, 6)]
    # Results arrive in completion order; leaving the block cancels the rest.
    with pool.run_many(queries, concurrency=2) as group:
        for done in group:
            print(f"  query #{done.index} -> {done.result()}")


async def async_examples(pool: fbfast.Pool) -> None:
    many = [Query.fetch_val("SELECT CAST(? AS INTEGER) FROM RDB$DATABASE", [n])
            for n in range(300)]  # more than pool_size + max_waiters is fine here
    results = await pool.run_all_async(many, timeout=30.0)
    print(f"run_all_async: {len(results)} results, sum={sum(results[q] for q in many)}")

    async with pool.run_many_async(many[:3]) as group:
        async for done in group:
            print(f"  async query #{done.index} -> {done.result()}")


def main() -> None:
    config = fbfast.FirebirdConfig(pool_size=4, min_idle=2)
    with fbfast.Pool(config) as pool:
        run_all_example(pool)
        print("fail_fast=False:")
        fail_fast_off_example(pool)
        print("run_many:")
        run_many_example(pool)
        asyncio.run(async_examples(pool))


if __name__ == "__main__":
    main()
