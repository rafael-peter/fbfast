# Recipes

Short, complete patterns for common jobs. Every snippet uses the public API of
fbfast 0.3.0; the runnable versions live in
[`examples/`](../../examples).
Table and column names such as `ORDERS` stand for your own schema.

Conventions used below:

- Methods without a suffix block; methods ending in `_async` (and `aclose`) are
  coroutines.
- Parameters are positional `?` placeholders, passed as a list or tuple.
- Pools, snapshots and streams are always used as context managers, or closed
  explicitly in a `finally`/lifespan.
- `get_pool(name).method(...)` is the canonical way to reach a named pool; the
  module functions such as `fbfast.fetch_all(sql, pool=name)` are shortcuts for it.

## FastAPI with a named pool

Open the pool once in the application lifespan, reach it by name in routes,
and close it on shutdown.

```python
from contextlib import asynccontextmanager

from fastapi import FastAPI
from pydantic import BaseModel, Field

import fbfast


class Order(BaseModel):
    id: int = Field(validation_alias="ID")
    customer: str = Field(validation_alias="CUSTOMER")


@asynccontextmanager
async def lifespan(app: FastAPI):
    # FirebirdConfig() reads FB_* variables (or FB_REPORTS_* with from_env).
    await fbfast.init_pool_async(fbfast.FirebirdConfig(), name="reports")
    try:
        yield
    finally:
        await fbfast.close_pool_async("reports")


app = FastAPI(lifespan=lifespan)


def reports() -> fbfast.Pool:
    return fbfast.get_pool("reports")  # never opens a pool implicitly


@app.get("/orders/count")
async def count_orders(status: str = "OPEN") -> int:
    return await reports().fetch_val_async(
        "SELECT COUNT(*) FROM ORDERS WHERE STATUS = ?", [status]
    )


@app.get("/orders/{order_id}")
async def get_order(order_id: int) -> Order | None:
    return await reports().fetch_one_async(
        "SELECT ID, CUSTOMER FROM ORDERS WHERE ID = ?", [order_id], model=Order
    )
```

- `init_pool_async` warms the pool (opens `min_idle` connections) and fails
  fast if the database is unreachable, so a misconfigured service does not start.
- Run one pool per process. With several Uvicorn/Gunicorn workers, the lifespan
  runs in each worker after it was forked, which is what fbfast requires.
- Every `*_async` call runs off the event loop. More concurrent requests than
  `pool_size + max_waiters` raise `PoolBusyError` immediately; map it to HTTP 503.
- Synchronous scripts use the same registry: `fbfast.init_pool(config, name=...)`
  and `fbfast.close_pool(name)`.

## NDJSON streaming over HTTP

`stream_json_async` writes newline-delimited JSON in Rust and yields `bytes`
chunks that always end on a complete record. Keep the stream context **inside**
the response generator, so it opens when the body starts and closes when the
body ends or the client disconnects.

```python
from fastapi.responses import StreamingResponse


@app.get("/orders.ndjson")
async def export_orders(status: str = "OPEN") -> StreamingResponse:
    async def body():
        async with reports().stream_json_async(
            "SELECT ID, CUSTOMER, TOTAL, CREATED_AT FROM ORDERS WHERE STATUS = ?",
            [status],
            timeout=None,  # no total deadline; stream_idle_timeout still applies
        ) as chunks:
            async for chunk in chunks:
                yield chunk

    return StreamingResponse(body(), media_type="application/x-ndjson")
```

- `query_timeout` (30 s by default) covers the whole stream, including the time
  the client takes to read it. Pass `timeout=` for long exports. A consumer that
  stops reading is bounded separately by `stream_idle_timeout`.
- An error after the headers were sent can only terminate the body; it cannot
  become an error status. Validate parameters before returning the response.
- Exact decimals are JSON strings and BIGINT/INT128 are strings by default
  (`json_decimals="string"`, `json_integers="safe"`), so JavaScript clients do
  not lose precision. Pass `"number"` to either option to emit numbers.
- For a single JSON document use `fetch_json_async` (returns `bytes`) and
  `Response(content=..., media_type="application/json")`.

## ETL to Polars and Parquet

`fetch_polars` builds the DataFrame through Arrow without Python rows and
without requiring PyArrow:

```python
from datetime import datetime

import fbfast

with fbfast.Pool(fbfast.FirebirdConfig()) as pool:
    frame = pool.fetch_polars(
        "SELECT ID, CUSTOMER, TOTAL, CREATED_AT FROM ORDERS WHERE CREATED_AT >= ?",
        [datetime(2026, 1, 1)],  # a TIMESTAMP parameter takes a datetime, not a str
    )
    frame.write_parquet("orders.parquet")
```

When the result does not fit comfortably in memory, stream Arrow batches into
the Parquet writer instead (requires `fbfast[arrow]`):

```python
import pyarrow as pa
import pyarrow.parquet as pq

with pool.stream_arrow("SELECT ID, CUSTOMER, TOTAL FROM ORDERS", batch_rows=50_000) as stream:
    reader = pa.RecordBatchReader.from_stream(stream)
    with pq.ParquetWriter("orders.parquet", reader.schema) as writer:
        for batch in reader:
            writer.write_batch(batch)
```

- Install the extras you need: `fbfast[polars]`, `fbfast[arrow]`, or both.
- `SUM()` over `NUMERIC(18, x)` or `BIGINT`, and `BIGINT * BIGINT`, are INT128
  in Firebird 4+, which Arrow represents as Decimal256. Polars cannot hold
  Decimal256, so `fetch_polars` raises `UnsupportedTypeError` unless you
  `CAST(SUM(TOTAL) AS NUMERIC(18, 2))` in SQL or pass `unsupported="string"`
  to receive exact decimal text.
- `max_result_bytes` (512 MiB by default) bounds `fetch_polars`/`fetch_arrow`;
  streams are bounded by `batch_bytes` and `prefetch_batches` instead.

## Arrow streaming

`stream_arrow` yields owned `ArrowBatch` objects. They implement the Arrow C
Data Interface, so PyArrow, Polars and other Arrow consumers can read them
without copying, and they remain valid after the stream closes.

```python
import polars as pl

with pool.stream_arrow("SELECT ID, TOTAL FROM ORDERS", batch_rows=10_000) as batches:
    print(batches.schema.names)  # available right after entering, even for empty results
    for batch in batches:
        frame = pl.DataFrame(batch)  # or pyarrow.record_batch(batch)
        process(frame)
```

The async variant iterates batches; it has no blocking C Stream export:

```python
async with pool.stream_arrow_async("SELECT ID, TOTAL FROM ORDERS") as batches:
    async for batch in batches:
        await process_async(batch)  # ArrowBatch: num_rows, num_columns, nbytes, schema
```

Use either iteration or the C Stream export
(`pa.RecordBatchReader.from_stream(stream)`) on a given sync stream, not both.

## A report with a snapshot

A snapshot reserves one connection with a READ ONLY SNAPSHOT transaction, so
every query inside sees the same committed data. It is also the fastest way to
run many small sequential queries over a slow link: each costs about one round
trip.

```python
with pool.snapshot(timeout=120.0) as snap:
    totals = snap.fetch_one(
        "SELECT COUNT(*) AS ORDERS, SUM(TOTAL) AS REVENUE FROM ORDERS WHERE STATUS = ?",
        ["PAID"], as_dict=True,
    )
    by_customer = snap.fetch_all(
        "SELECT CUSTOMER, SUM(TOTAL) AS REVENUE FROM ORDERS WHERE STATUS = ? "
        "GROUP BY CUSTOMER ORDER BY 2 DESC ROWS 10",
        ["PAID"],
    )
    with snap.stream("SELECT ID, TOTAL FROM ORDERS WHERE STATUS = ?", ["PAID"]) as batches:
        for rows in batches:
            write_detail(rows)
```

The same code works asynchronously with `async with pool.snapshot() as snap:`
and the `*_async` methods.

- Only one operation runs at a time inside a snapshot, including an open stream;
  a second one raises `ConcurrentUseError`. Do not share a snapshot between
  threads or tasks; use `run_all` on the pool for parallel work.
- The snapshot deadline (`snapshot_timeout`, 300 s by default, or `timeout=`)
  counts the time your code spends between queries. Each query inside still
  has its own `query_timeout`.
- Any error ends the snapshot: later calls on it, and leaving the `with` block,
  raise that error again. Handle errors outside the block and open a new snapshot.

## Pydantic models

Pass `model=` to `fetch_all`/`fetch_one` (and their async versions, snapshots
and `Query` objects). The return type follows the class: `list[Order]` and
`Order | None`.

```python
from decimal import Decimal

from pydantic import BaseModel, Field


class Order(BaseModel):
    id: int = Field(validation_alias="ID")
    customer: str = Field(validation_alias="CUSTOMER")
    total: Decimal = Field(validation_alias="TOTAL")


orders = pool.fetch_all("SELECT ID, CUSTOMER, TOTAL FROM ORDERS WHERE STATUS = ?",
                        ["OPEN"], model=Order)
```

- Install with `pip install "fbfast[pydantic]"` (Pydantic v2).
- Firebird returns unquoted names in UPPER CASE (`ID`, `CUSTOMER`). Either name
  the fields that way or map them with `Field(validation_alias=...)`, as above.
  Column names are never renamed silently.
- Every column name must be unique: alias expressions and joined columns
  (`SELECT O.ID, C.ID AS CUSTOMER_ID ...`), or the call raises `DataConversionError`.
- Validation runs after fetching, outside the query deadline; async validation
  runs in a worker thread. A row that fails raises `pydantic.ValidationError`.
- Streams do not accept `model=`; validate each batch yourself
  (`stream(..., as_dict=True)` + `Order.model_validate(row)`).

## Several queries at once

Describe each query with a `fbfast.Query` factory, then run them together.
At most `concurrency` queries hold a connection at once (default and maximum:
`pool_size`); the rest wait outside the pool queue, so other callers keep being
served and there is no limit on the number of queries.

```python
from fbfast import Query

open_orders = Query.fetch_all("SELECT ID, CUSTOMER, TOTAL FROM ORDERS WHERE STATUS = ?",
                              ["OPEN"], model=Order)
revenue = Query.fetch_val("SELECT SUM(TOTAL) FROM ORDERS WHERE STATUS = ?", ["PAID"])
daily = Query.fetch_polars("SELECT CAST(CREATED_AT AS DATE) AS ORDER_DAY, COUNT(*) AS N "
                           "FROM ORDERS GROUP BY 1")

results = pool.run_all([open_orders, revenue, daily], timeout=30.0)
results[open_orders]  # list[Order], typed by the Query
results[revenue]      # the value (fetch_val is typed Any)
results[daily]        # polars.DataFrame
```

Handle each result as soon as it finishes with `run_many`:

```python
with pool.run_many([open_orders, revenue, daily], concurrency=2) as group:
    for done in group:  # completion order
        print(done.index, done.query.sql, done.result())
```

And in async code:

```python
results = await pool.run_all_async([open_orders, revenue, daily])

async with pool.run_many_async(queries) as group:
    async for done in group:
        handle(done.query, done.result())
```

- Factories: `Query.fetch_all`, `fetch_one`, `fetch_val`, `fetch_json`,
  `fetch_arrow`, `fetch_polars`, `explain`. Streams are not available in groups.
- Each `Query` object is a distinct key; create two objects to run the same SQL
  twice. Passing the same object twice raises `ArgumentError`.
- `fail_fast=True` (default) cancels the rest on the first error and raises it;
  `fail_fast=False` runs everything and collects failures in `results.errors`.
- `timeout=` is a deadline for the whole group and cancels what is unfinished;
  each `Query` keeps its own `timeout=`. Leaving a `run_many` block cancels
  unfinished queries.
- Prefer `run_all_async` over `asyncio.gather` of many `*_async` calls: a plain
  `gather` of more than `pool_size + max_waiters` (264 by default) calls fails
  with `PoolBusyError`.
- Groups run on the pool, not inside a snapshot. Each query uses its own
  transaction, so the results are not a consistent snapshot of each other.

## See also

- [Errors](errors.md) and [troubleshooting](troubleshooting.md)
- [Configuration](configuration.md), [performance](performance.md) and the
  [API reference](api.md)
