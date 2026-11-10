# fbfast

**Firebird reads, ready for Python applications and data pipelines.**

fbfast is a read-only Firebird 5 client with a Rust execution core and the
official Firebird client underneath. Query once, choose the representation your
application needs: Python rows, validated Pydantic models, native JSON, Arrow,
or Polars. Use the same API in synchronous scripts and asynchronous services.

```python
from fbfast import FirebirdConfig, Pool
from pydantic import BaseModel

class Order(BaseModel):
    ID: int
    CUSTOMER: str

with Pool(FirebirdConfig()) as pool:
    orders = pool.fetch_all(
        "SELECT ID, CUSTOMER FROM ORDERS WHERE STATUS = ?",
        ["OPEN"],
        model=Order,
    )
    # Inferred type: list[Order]. Fields are validated before returning.
    for order in orders:
        print(order.ID, order.CUSTOMER)
```

## Why fbfast?

- **One client, multiple destinations.** Fetch application objects, send JSON to
  an HTTP consumer, or hand native Arrow buffers to your analytics stack.
- **Built for slow links.** A pooled query usually answers after a single
  network round trip, small BLOBs travel inside their rows, and independent
  queries run together with `run_all`.
- **Less Python work for bulk data.** JSON and Arrow are built in Rust without
  materializing an intermediate table of Python tuples or dictionaries.
- **Explicit resource control.** Bounded connection pools, statement caching,
  deadlines, cancellation, and streaming with backpressure.
- **Precision that survives the trip.** Exact decimals, wide integers, BLOBs,
  and explicit timezone handling, with documented conversion rules.
- **Async without a second API to learn.** Await `*_async` methods and use
  `async with` for pools, snapshots, streams and query groups.
- **Wheels with the client included.** Consumers of a compatible wheel do not
  need Rust, a C++ compiler, or a separate Firebird client installation.

Built for reporting, read APIs, exports, and analytics over existing Firebird
systems. fbfast is not an ORM or a write driver. It uses read-only transactions;
use a database account and role with appropriate read permissions as well.

## Install

```sh
pip install fbfast                 # or: uv add fbfast
pip install "fbfast[pydantic]"     # typed results with Pydantic v2
pip install "fbfast[arrow]"        # PyArrow output
pip install "fbfast[polars]"       # Polars output; PyArrow is not required
```

With uv: `uv add "fbfast[pydantic]"`, and likewise for the other extras. All
extras are optional: importing fbfast and fetching ordinary rows never imports
Pydantic, PyArrow or Polars.

| Platform | Architectures | Python |
| --- | --- | --- |
| macOS | Apple Silicon, Intel | CPython 3.11–3.15 |
| Linux (glibc) | aarch64, x86_64 | CPython 3.11–3.15 |
| Windows | x86_64 | CPython 3.11–3.15 |

Wheels are specific to the OS, architecture, and Python ABI. Free-threaded
Python and `abi3` wheels are not supported. A reachable Firebird 5.0.x server is
required for queries; 5.0.4 or newer is recommended.

## Connect in seconds

Set your environment variables, or put them in a local `.env`:

```dotenv
FB_HOST=localhost
FB_DATABASE=/path/on/server/application.fdb
FB_USER=REPORT_READER
FB_PASSWORD=your-password
# FB_ROLE=REPORTING
```

```python
from fbfast import FirebirdConfig, Pool

with Pool(FirebirdConfig()) as pool:
    row = pool.fetch_one("SELECT CURRENT_TIMESTAMP FROM RDB$DATABASE")
    print(row)
```

Configuration precedence is **explicit argument → environment → `.env` →
default**. Database, user, and password are required. `env_file=False` disables
`.env` loading; `FirebirdConfig.from_env("FB_BI_")` reads another prefix.
Configuration is immutable and its representation hides the password.

```python
config = FirebirdConfig(
    pool_size=4,
    min_idle=1,
    query_timeout=30.0,
    max_result_rows=100_000,
    env_file=False,
)
```

Omitted query timeouts inherit `query_timeout` (30 s); `timeout=None` disables
an individual query deadline. `max_result_bytes` (512 MiB by default) and
`max_result_rows` bound each materialized result; streams use `batch_bytes`,
`prefetch_batches` and backpressure instead.

Wire encryption defaults to `wire_crypt="required"`, with no silent downgrade.
`"enabled"` negotiates encryption but permits an unencrypted connection;
`"disabled"` disables it. Connections use UTF8; column charsets are converted by
the server. Every option, its `FB_*` variable and its default are listed in the
[configuration guide](https://github.com/rafael-peter/fbfast/blob/main/docs/guide/configuration.md).

## Security boundary

fbfast rejects common write statements lexically, verifies the prepared
statement type, and opens Firebird `READ ONLY` transactions. These three gates
reduce accidental writes, but they do not turn arbitrary SQL into a sandbox.
Selectable procedures, functions, UDRs, generators, and temporary tables can
have effects that transaction rollback does not undo.

Run the library with a dedicated database account and a minimal role containing
only the required `SELECT` grants. If SQL comes from an HTTP endpoint, an end
user, or a language model, place an allowlist of application queries above the
driver. See the complete
[security model](https://github.com/rafael-peter/fbfast/blob/main/docs/security.md).

## Choose your result

| Method | Result | Best suited to |
| --- | --- | --- |
| `fetch_one` | Tuple, dictionary, model, or `None` | One application record |
| `fetch_all` | List of tuples, dictionaries, or models | Bounded result sets |
| `fetch_val` | First column of the first row, or `None` | Counts and scalar values |
| `fetch_json` | UTF-8 JSON `bytes` | HTTP responses and exports |
| `fetch_arrow` | `pyarrow.Table` | Columnar processing |
| `fetch_polars` | `polars.DataFrame` | Data analysis |
| `stream` | Batches of tuples or dictionaries | Incremental row processing |
| `stream_json` | NDJSON or JSON-array byte chunks | Incremental delivery |
| `stream_arrow` | Owned `ArrowBatch` objects | Columnar streaming |

Every method has an `_async` counterpart. Bind values with `?` placeholders,
using the Python type that matches the column (`datetime` for `TIMESTAMP`,
`Decimal` for `NUMERIC`...):

```python
rows = pool.fetch_all("SELECT ID FROM ORDERS WHERE ID > ?", [100])
row = pool.fetch_one("SELECT ID FROM ORDERS WHERE ID = ?", [101], as_dict=True)
count = pool.fetch_val("SELECT COUNT(*) FROM ORDERS")
plan = pool.explain("SELECT ID FROM ORDERS WHERE ID = ?", [101])
```

### Typed results with Pydantic

Pass the model class directly. The return type follows the class, including in
editor autocomplete and static type checking:

```python
from pydantic import BaseModel, Field

class Order(BaseModel):
    id: int = Field(validation_alias="ID")
    customer: str = Field(validation_alias="CUSTOMER")

order = pool.fetch_one("SELECT ID, CUSTOMER FROM ORDERS WHERE ID = ?", [101], model=Order)
# Order | None

orders = await pool.fetch_all_async("SELECT ID, CUSTOMER FROM ORDERS", model=Order)
# list[Order]
```

Firebird returns unquoted column names in upper case, and fbfast never renames
them: name fields in upper case or use validation aliases. Column names must be
unique. Validation runs after fetching, outside the query deadline (in a thread
for async calls); `pydantic.ValidationError` propagates and rows are never
skipped silently. The same `model=` option works on snapshots and query groups.

## Async services and named pools

```python
from contextlib import asynccontextmanager

import fbfast
from fastapi import FastAPI

@asynccontextmanager
async def lifespan(app: FastAPI):
    await fbfast.init_pool_async(fbfast.FirebirdConfig(), name="reports")
    try:
        yield
    finally:
        await fbfast.close_pool_async("reports")

app = FastAPI(lifespan=lifespan)

@app.get("/order-count")
async def order_count():
    return await fbfast.get_pool("reports").fetch_val_async("SELECT COUNT(*) FROM ORDERS")
```

`get_pool(name)` returns an explicitly initialized pool from anywhere in the
process; queries never create a hidden global pool. Omit the name to use
`"default"`. Synchronous applications use `init_pool()` and `close_pool()`.
Module functions such as `fbfast.fetch_val(sql, pool="reports")` are shortcuts
for `get_pool("reports").fetch_val(sql)`.

The lifecycle follows one rule: no suffix blocks, `_async` (and `aclose`) is a
coroutine. `pool.close()` / `await pool.aclose()`, `pool.warmup()` /
`await pool.warmup_async()`. Code written for 0.2 that awaits `pool.close()`
now fails with `TypeError`; the old `*_sync` names still work with a
`DeprecationWarning` until 0.4.0.

Create pools after the process starts (after `fork()`); inherited pools are
rejected in the child.

## Several queries at once

Describe each query with `fbfast.Query`, then run them together. At most
`pool_size` queries hold a connection at a time; the rest wait outside the pool
queue, so other callers keep being served.

```python
from fbfast import Query

orders = Query.fetch_all("SELECT ID, CUSTOMER FROM ORDERS WHERE STATUS = ?", ["OPEN"],
                         model=Order)
total = Query.fetch_val("SELECT SUM(TOTAL) FROM ORDERS")

results = pool.run_all([orders, total], timeout=30.0)
results[orders]  # list[Order]

with pool.run_many([orders, total]) as group:  # in completion order
    for done in group:
        print(done.query.sql, done.result())

results = await pool.run_all_async([orders, total])
```

With 50 ms of latency, N small queries take about `ceil(N / pool_size)` times
one query. `fail_fast=True` (the default) cancels the rest on the first error;
leaving a `run_many` block cancels unfinished queries.

## Stream without retaining the entire result

```python
with pool.stream("SELECT ID, CUSTOMER FROM ORDERS", batch_rows=2_000) as batches:
    for rows in batches:
        process_batch(rows)  # Your application callback

async with pool.stream_json_async(
    "SELECT ID, CUSTOMER FROM ORDERS", format="ndjson"
) as chunks:
    async for chunk in chunks:
        await send_chunk(chunk)  # Your transport callback
```

Always use the context manager, especially when stopping early. Streams are
lazy, single-consumer resources. The query deadline includes consuming the
stream; `stream_idle_timeout` bounds how long the producer waits for you. For
HTTP streaming, keep the stream context **inside the response generator**.

`fetch_json` returns a compact JSON array. Exact decimals and BIGINT/INT128 are
JSON strings by default (`json_decimals="string"`, `json_integers="safe"`) so
JavaScript consumers do not lose precision; pass `"number"` to emit numbers.

### Arrow and Polars

```python
table = pool.fetch_arrow("SELECT ID, AMOUNT FROM ORDERS")
frame = await pool.fetch_polars_async("SELECT ID, AMOUNT FROM ORDERS")

import pyarrow as pa

with pool.stream_arrow("SELECT ID, AMOUNT FROM ORDERS") as stream:
    for batch in pa.RecordBatchReader.from_stream(stream):
        process_arrow_batch(batch)
```

Arrow preserves described SQL types, even for empty results. INT128-based
values (including `SUM()` over `BIGINT` or `NUMERIC(18, x)`) use Decimal256,
which Polars cannot hold: `fetch_polars` raises unless you `CAST` in SQL or pass
`unsupported="string"` for exact decimal text. `stream_arrow` batches implement
the Arrow C Data Interface and need neither PyArrow nor Polars.

## Consistent reads and precise types

Use a snapshot when several queries must see the same database state:

```python
with pool.snapshot(timeout=120.0) as session:     # or: async with pool.snapshot()
    total = session.fetch_val("SELECT COUNT(*) FROM ORDERS")
    orders = session.fetch_all("SELECT ID, CUSTOMER FROM ORDERS", model=Order)
```

A snapshot reserves one connection and accepts one operation at a time. Its
deadline (`snapshot_timeout`, 300 s by default) includes the time between
queries. Any error ends the session. Each query costs about one round trip, so a
snapshot is also the fastest way to run many small sequential queries over a
slow link.

- NUMERIC/DECIMAL and INT128 preserve exact precision. DECFLOAT uses
  `decimal.Decimal`, including signed zero and special values. Binding rejects
  precision loss rather than silently rounding.
- Naive SQL dates/times remain naive. Temporal precision is 100 microseconds.
- TIMESTAMP WITH TIME ZONE maps to aware `datetime`; TIME WITH TIME ZONE maps to
  `TimeWithZone(utc_time, zone)`.
- Text BLOBs become `str` and binary BLOBs `bytes`, bounded by `max_blob_bytes`.

## Latency and diagnostics

fbfast 0.3.0 is tuned for databases reached over a WAN, VPN or tunnel:

- A cached query in a pool answers after **1 round trip** (execute plus the
  first batch) instead of 4–6. After each complete result, the session reset
  (`ALTER SESSION RESET`) also starts the next query's transaction, all after
  the reply and while the connection stays reserved. A transaction left unused
  for `ready_transaction_timeout` (2 s) is rolled back; then the next query
  costs 2 round trips (begin, execute). A failure in that cleanup discards the
  connection and is counted in `stats()["cleanup_errors"]`.
- Idle connections are pinged in the background after `health_check_after`,
  which also keeps tunnels and NAT mappings alive. A connection that fails
  before any SQL is sent is replaced once; SQL already sent is never repeated.
- With Firebird 5.0.3+ servers, BLOBs up to 65,535 bytes travel inside the row.
  At 20 ms of latency, 1,000 rows with a small text BLOB went from 28.3 s in
  0.2.1 to 0.19 s (firebird-driver: 0.22 s), and 32 small queries run together
  from 0.47 s to 0.19 s.
- `wire_compression=True` requests zlib compression on every platform; check
  `session_info()["wire_compressed"]` to see whether the server agreed.

```python
config = FirebirdConfig(diagnostics=True)
with Pool(config) as pool:
    pool.fetch_all("SELECT ID FROM ORDERS")
    print(pool.diagnostics()[-1])  # seconds and round trips per phase
    print(pool.stats())            # counters, including wire_roundtrips
    print(pool.session_info())     # wire_encrypted, wire_compressed, inline_blobs...
```

After DDL changes a table, call `pool.clear_statement_cache()` (or
`await pool.clear_statement_cache_async()`), or recreate the pool.

fbfast does not claim that Rust makes every query faster: on a local server the
database usually dominates. Measurements, methodology and raw results are in the
[performance guide](https://github.com/rafael-peter/fbfast/blob/main/docs/guide/performance.md)
and [benchmarks](https://github.com/rafael-peter/fbfast/tree/main/benchmarks).

## Documentation

- [Recipes](https://github.com/rafael-peter/fbfast/blob/main/docs/guide/recipes.md):
  FastAPI, NDJSON over HTTP, Polars/Parquet ETL, Arrow streaming, reports,
  Pydantic, concurrent queries
- [Errors](https://github.com/rafael-peter/fbfast/blob/main/docs/guide/errors.md)
  and [troubleshooting](https://github.com/rafael-peter/fbfast/blob/main/docs/guide/troubleshooting.md)
- [Configuration](https://github.com/rafael-peter/fbfast/blob/main/docs/guide/configuration.md),
  [performance](https://github.com/rafael-peter/fbfast/blob/main/docs/guide/performance.md)
  and the [API reference](https://github.com/rafael-peter/fbfast/blob/main/docs/guide/api.md)
- [Runnable examples](https://github.com/rafael-peter/fbfast/tree/main/examples)
- [Release notes](https://github.com/rafael-peter/fbfast/tree/main/docs/releases)
- For code assistants: [`llms.txt`](https://github.com/rafael-peter/fbfast/blob/main/llms.txt);
  the installed package also ships `fbfast/llms.txt`.

## Development

Building from source, checks and the disposable test fixture are described in
[docs/development.md](https://github.com/rafael-peter/fbfast/blob/main/docs/development.md).

## License

fbfast is licensed under either of
[Apache License 2.0](https://github.com/rafael-peter/fbfast/blob/main/LICENSE-APACHE) or
[MIT license](https://github.com/rafael-peter/fbfast/blob/main/LICENSE-MIT), at your option
(`MIT OR Apache-2.0`). The bundled official Firebird client and its libraries
keep their own licenses (IPL and IDPL for Firebird); wheels include them under
`fbfast/_licenses/`.
