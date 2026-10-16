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

> The `model=` API was introduced in 0.2.0. The 0.2.1 security update is
> available from [PyPI](https://pypi.org/project/fbfast/0.2.1/).

## Why fbfast?

- **One client, multiple destinations.** Fetch application objects, send JSON to
  an HTTP consumer, or hand native Arrow buffers to your analytics stack.
- **Less Python work for bulk data.** JSON and Arrow are built in Rust without
  materializing an intermediate table of Python tuples or dictionaries.
- **Explicit resource control.** Bounded connection pools, statement caching,
  deadlines, cancellation, and streaming with backpressure.
- **Precision that survives the trip.** Exact decimals, wide integers, BLOBs,
  and explicit timezone handling, with documented conversion rules.
- **Async without a second API to learn.** Await `*_async` methods and use
  `async with` for pool, snapshot, and stream lifecycles.
- **Wheels with the client included.** Consumers of a compatible wheel do not
  need Rust, a C++ compiler, or a separate Firebird client installation.

Built for reporting, read APIs, exports, and analytics over existing Firebird
systems. fbfast is not an ORM or a write driver. It uses read-only transactions;
use a database account and role with appropriate read permissions as well.

## Install

```sh
uv add fbfast                  # or: pip install fbfast
uv add 'fbfast[arrow]'         # PyArrow output
uv add 'fbfast[polars]'        # Polars output; PyArrow is not required
```

For typed models in this source revision, build a wheel as described under
[Development](#development), then install it with the Pydantic extra:

```sh
pip install '/absolute/path/to/fbfast-VERSION-PLATFORM.whl[pydantic]'
```

Pydantic v2 is optional. Importing fbfast and fetching ordinary rows do not
import Pydantic. Arrow and Polars are also optional dependencies.

| Platform | Architectures | Python |
| --- | --- | --- |
| macOS | Apple Silicon, Intel | CPython 3.11–3.14 |
| Linux | aarch64, x86_64 | CPython 3.11–3.14 |
| Windows | x86_64 | CPython 3.11–3.14 |

Wheels are specific to the OS, architecture, and Python ABI. Free-threaded
Python and `abi3` wheels are not currently supported. A reachable Firebird 5
server is required for queries. See the [release guide](docs/releasing.md) and
[recorded platform validation](docs/progress.md).

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
`.env` loading; `from_env(prefix="FB_BI_")` supports a custom environment prefix.
Configuration is immutable and its representation hides the password.

```python
config = FirebirdConfig(
    pool_size=4,
    min_idle=1,
    query_timeout=30.0,
    max_result_rows=100_000,
    max_result_bytes=512 * 1024 * 1024,
    env_file=False,
)
```

Omitted query timeouts inherit `query_timeout`; `timeout=None` disables the
individual query deadline. `max_result_rows` defaults to `None` (unlimited)
and applies to materialized results. `max_result_bytes` defaults to 512 MiB and
limits each result materialized as rows, dictionaries, Pydantic models, JSON,
Arrow, or Polars. Set it to `None` only when the caller provides another bound. It is an
accounting limit for retained core buffers, not an exact process-RSS ceiling;
temporary conversion buffers and concurrent results can use more memory.
Streams use `batch_bytes`, `prefetch_batches`, and backpressure instead of a
cumulative byte limit.

Wire encryption defaults to `wire_crypt="required"`, with no silent downgrade.
`"enabled"` negotiates encryption but permits an unencrypted connection;
`"disabled"` disables it. Boolean `True`/`False` map to required/disabled.
The current executor requires UTF8 connections.

## Security boundary

fbfast rejects common write statements lexically, verifies the prepared
statement type, and opens Firebird `READ ONLY` transactions. These three gates
reduce accidental writes, but they do not turn arbitrary SQL into a sandbox.
Selectable procedures, functions, UDRs, generators, and temporary tables can
have effects that transaction rollback does not undo.

Run the library with a dedicated database account and a minimal role containing
only the required `SELECT` grants. Do not grant unnecessary `EXECUTE` or
`USAGE`. If SQL comes from an HTTP endpoint, an end user, or a language model,
place an allowlist of application queries above the driver. See the complete
[security model and deployment guidance](docs/security.md).

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

All methods above have async counterparts. Bind values with `?` placeholders:

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

orders = pool.fetch_all("SELECT ID, CUSTOMER FROM ORDERS", model=Order)
# list[Order]

orders = await pool.fetch_all_async("SELECT ID, CUSTOMER FROM ORDERS", model=Order)
# list[Order]
```

`model=` selects column-name dictionaries internally; `as_dict=True` is
redundant when a model is supplied. Column names are not silently renamed: use
SQL aliases or Pydantic validation aliases. Empty results produce `None` or
`[]`. Pydantic's `model_validate` applies your model's coercion, strictness,
extra-field policy, and custom validators. Validation errors propagate as
`pydantic.ValidationError`; invalid rows are not silently skipped.

The same option works on snapshots and module-level named-pool helpers.
Streaming does not yet accept `model=`.

Validation happens after fetching, outside the query deadline. Ordinary pool
queries have completed their database work at that point; an explicit snapshot
still owns its connection until its context closes. Async model validation runs
in a thread, so validators must be safe to run outside the calling thread.
Cancelling the await does not forcibly stop a validator already running.

Model construction costs CPU and memory compared with raw rows. The conversion
is comparable to manually validating each dictionary; no throughput improvement
is claimed. Calls without `model=` keep the native row-return path.

## Async services and named pools

```python
import fbfast
from contextlib import asynccontextmanager
from fastapi import FastAPI

@asynccontextmanager
async def lifespan(app: FastAPI):
    await fbfast.init_pool(fbfast.FirebirdConfig(), name="reports")
    try:
        yield
    finally:
        await fbfast.close_pool("reports")

app = FastAPI(lifespan=lifespan)

@app.get("/order-count")
async def order_count():
    return await fbfast.fetch_val_async("SELECT COUNT(*) FROM ORDERS", pool="reports")
```

Named helpers resolve an explicitly initialized pool; queries never create a
hidden global pool. Omit the name to use `"default"`. Synchronous applications
can use `init_pool_sync` and `close_pool_sync`.

Initialize pools after process creation. Inherited pools are rejected after
fork. Closing drains the pool; if cleanup times out or is cancelled, retry
closing before reusing the name. Native calls stuck in network/attach work may
outlive a caller's timeout, and still count against the pool capacity.

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
lazy, single-consumer resources. `batch_rows` defaults to `fetch_size`;
`batch_bytes` also bounds batches, although one oversized record may occupy its
own batch. `prefetch_batches` bounds lookahead; zero disables prefetch.
Retaining batches in your own code still consumes memory.

The query deadline includes stream consumption and native row conversion;
`stream_idle_timeout` bounds producer idle time. `timeout=None` does not disable
the idle timeout. Cleanup errors propagate before successful end-of-stream.
For HTTP streaming, keep the stream context **inside the response generator**.
An error after headers have been sent terminates the body; it cannot replace
the response with a new error status.

### Native JSON

`fetch_json` returns a compact JSON array (`b"[]"` when empty). Streaming supports
`format="ndjson"` for complete newline-delimited records or `format="array"`
for one JSON document split across chunks. Array chunks are not individually
parseable documents. An interrupted array is incomplete.

| Option | Default | Alternative |
| --- | --- | --- |
| `orient` | `"records"` — unique column names | `"values"` — arrays |
| `json_integers` | `"safe"` — BIGINT/INT128 as strings | `"number"` |
| `json_decimals` | `"string"` — exact decimal text | `"number"` — exact numeric tokens |
| `json_binary` | `"error"` | `"base64"` |
| `json_non_finite` | `"error"` | `"null"` |

SMALLINT/INTEGER remain JSON numbers. Numeric JSON tokens can exceed a
consumer's precision, particularly in JavaScript. The native serializer does
not route exact decimals through floating point.

### Arrow and Polars

```python
table = pool.fetch_arrow("SELECT ID, AMOUNT FROM ORDERS")
frame = await pool.fetch_polars_async("SELECT ID, AMOUNT FROM ORDERS")
```

Arrow preserves described SQL types even for empty or initially NULL results,
and carries SQL metadata on fields. INT128 decimals use Decimal256. Polars
imports Arrow buffers with `rechunk=False`; unsupported Decimal256 columns
raise by default. Set `unsupported="string"` for an explicit exact-text fallback.
Use distinct SQL aliases for duplicate column names.

`stream_arrow` yields owned batches with `num_rows`, `num_columns`, `nbytes`,
`schema`, and the Arrow C Array interface. It requires neither PyArrow nor
Polars. Retained batches remain valid after the stream closes.

```python
import pyarrow as pa

with pool.stream_arrow("SELECT ID, AMOUNT FROM ORDERS") as stream:
    reader = pa.RecordBatchReader.from_stream(stream)
    for batch in reader:
        process_arrow_batch(batch)
```

The blocking C Stream export is single-use and exclusive with Python iteration.
Async Arrow streams use batch iteration instead of a blocking C Stream export.
See the [Arrow contracts](docs/decisions/0017-arrow-streams.md).

## Consistent reads and precise types

Use a snapshot when several queries must see the same database state:

```python
with pool.snapshot(timeout=120.0) as session:
    total = session.fetch_val("SELECT COUNT(*) FROM ORDERS")
    orders = session.fetch_all("SELECT ID, CUSTOMER FROM ORDERS", model=Order)
```

A snapshot reserves one connection and accepts one operation at a time. Its
deadline includes time between queries; disabling an individual query timeout
does not disable the session deadline. Errors/cancellation of native operations
end the session. Queries on the parent pool use independent transactions.
Use `async with pool.snapshot_async(...)` for async access.

- NUMERIC/DECIMAL and INT128 preserve supported exact precision. DECFLOAT uses
  `decimal.Decimal`, including signed zero and special values. Binding rejects
  precision loss rather than silently rounding.
- Naive SQL dates/times remain naive. Temporal precision is 100 microseconds.
- TIMESTAMP WITH TIME ZONE maps to aware `datetime`; incompatible or missing
  Python zone rules raise a conversion error.
- TIME WITH TIME ZONE maps to immutable `TimeWithZone(utc_time, zone)` rather
  than inventing a calendar date. JSON uses explicit UTC/zone objects; Arrow
  uses structured timezone fields.
- BLOB conversion follows the configured text/binary policies and cell limits.

Detailed contracts: [BLOBs](docs/decisions/0013-transactional-blobs.md),
[DECFLOAT](docs/decisions/0014-exact-decfloat.md),
[timezones](docs/decisions/0015-time-zones.md), and
[snapshots](docs/decisions/0010-public-snapshots.md).

## Performance, with evidence

fbfast focuses on native output paths, bounded execution, and predictable
resource ownership. It does **not** claim that Rust makes every query faster.

In the recorded local comparison of 100,000 rows with two integer columns,
fbfast's median JSON time was 0.530 s versus 0.548 s for the tuned
`firebird-driver` baseline; reported JSON RSS was 36.5 versus 75.6 MiB. Tuple
time was 0.540 versus 0.516 s. These results describe one workload and environment,
not a universal speedup. They predate Pydantic support.

See the [benchmark methodology](benchmarks/README.md),
[recorded results](benchmarks/results/20261015T175028Z-comparison/), and
[validation history](docs/progress.md). Parallel extraction and wider performance
coverage remain on the [roadmap](docs/roadmap.md).

## Development

Source builds require Rust 1.91.1, a C++ toolchain, a supported CPython, and a
Firebird 5.0.x SDK/runtime (5.0.3 or newer). Published wheels bundle the pinned
Firebird 5.0.3 client and test servers 5.0.3 and 5.0.4. The build does not
download the client implicitly. Install development dependencies from
`requirements-dev.txt`.

```sh
python3.14 scripts/build_vendor.py --release --out dist/vendor \
  --client-root /Library/Frameworks/Firebird.framework
python3.14 scripts/verify_vendor.py --package dist/vendor/fbfast
```

Use the appropriate SDK path for your platform. The build writes wheels to
`dist/wheels/` and a complete copyable package to `dist/vendor/fbfast/`.
Install the wheel with uv/pip, or copy the entire vendor package to an importable
location. Do not mix files from different builds. Stop consumers before updating
loaded native libraries, and remove an old vendored copy when switching to pip.

```python
import fbfast

print(fbfast.build_info())   # Build/ABI metadata; no database connection
print(fbfast.client_info())  # Loads the client; no database connection
```

`pool.stats()` exposes pool counters. `pool.explain(sql, params)` returns the
execution plan. Before schema changes, stop new work, drain active queries, and
clear statement caches with `clear_statement_cache_sync()` / await
`clear_statement_cache()`, or recreate the pool. Cache invalidation does not
interrupt active cursors; cancelling the wait does not undo the request.

For repository checks and disposable-fixture tests, see
[development and build storage](docs/development.md). For release automation,
see [releasing](docs/releasing.md). Architecture decisions and historical
validation notes under `docs/` currently include Portuguese documentation.
