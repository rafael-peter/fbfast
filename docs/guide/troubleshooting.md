# Troubleshooting

Symptoms, causes and fixes for problems seen in practice. Each section names the
exception you get; [errors](errors.md) explains the hierarchy and its attributes.

Two tools answer most questions without touching the server again:

```python
info = pool.session_info()   # negotiated capabilities of the latest connection, or None
stats = pool.stats()          # pool counters (no I/O)
traces = pool.diagnostics()   # per-query phases; needs FirebirdConfig(diagnostics=True)
```

`session_info()` returns `server_version`, `role`, `charset`, `time_zone`,
`wire_encrypted`, `wire_compressed`, `network_protocol`, `protocol_version`,
`inline_blobs`, `max_inline_blob_size` and `blob_cache_size`.

## Connecting

### `AuthenticationError`: "Incompatible wire encryption levels requested on client and server"

`wire_crypt` defaults to `"required"`. A server with `WireCrypt = Disabled`
refuses it, and a server with the default `WireCrypt = Required` refuses a
client configured with `"disabled"`. Firebird reports this with SQLSTATE
`28000`, so it arrives as `AuthenticationError`; `native_codes` contains
`335545064`.

- Prefer enabling encryption on the server and keeping `wire_crypt="required"`.
- `wire_crypt="enabled"` negotiates encryption but accepts an unencrypted
  connection. Use it, or `"disabled"`, only on a transport you trust (for
  example a WireGuard/Tailscale tunnel). Check `session_info()["wire_encrypted"]`.
- `QueryError` (phase `attach`) "Effective wire encryption violates the
  configured policy" means the negotiated result contradicts your setting; fix
  the configuration rather than retrying.

### `AuthenticationError` with "user name and password are not defined"

Wrong user or password, or the server's `AuthServer` list does not include
`Srp256`/`Srp`, which the bundled client uses. `Legacy_Auth` is not offered.

### `RoleNotActiveError`: "Requested role is not active"

The session did not end up with the role you asked for. Either the name is
wrong, or the user lacks `GRANT <role> TO <user>`. Unquoted role names are
compared in upper case; pass a quoted name (`role='"MixedCase"'`) only for a
role created with a quoted identifier. `session_info()["role"]` shows the active
role; with `role=None` it is `NONE`.

### `DatabaseConnectionError` at `attach`

SQLSTATE `08001`/`08006` and messages such as "Unable to complete network
request" or "I/O error during open" mean the server, port or database path is
wrong or unreachable. `database` is the alias or the path **on the server**.
fbfast only supports remote TCP connections (`network_protocol` `TCPv4`/`TCPv6`),
including to `localhost`; embedded and XNET connections are rejected.

When the session handshake finds that the connection differs from the
configuration (charset, time zone or encryption policy), the error is a plain
`QueryError` with phase `attach` and no `sqlstate`: retrying does not help.

### `UnsupportedServerError`, `ClientLibraryError`

fbfast talks to Firebird 5.0.x servers only. The wheels bundle the official
5.0.3 client; `ClientLibraryError` means that library (or the one set with
`client_library`) could not be loaded. Reinstall the wheel for your platform
and Python version instead of copying files between environments.

## Character sets and text

### `UnsupportedFeatureError`: the connection charset must be UTF8

Only UTF8 connections are supported (`charset="UTF8"`, the default). This does
not limit your columns: the server converts WIN1252, ISO8859_1 and other column
charsets to UTF8 on the way out, and you receive `str`.

### Text comes back as `bytes`

Columns and expressions with charset `NONE` (some legacy tables, and functions
such as `RDB$GET_CONTEXT`) have no declared encoding, so fbfast returns bytes.
Set `fallback_charset="UTF8"` to decode them, or `CAST(... AS VARCHAR(n)
CHARACTER SET UTF8)` in SQL. Bytes that are not valid UTF-8 raise
`DataConversionError`.

### Trailing spaces in `CHAR` columns

`CHAR(n)` values keep their padding (system tables use `CHAR(63)` for names).
Use `TRIM()` in SQL or `trim_char=True`; `VARCHAR` is never trimmed.

## Parameters and columns

### `DataConversionError`: "Parameter 1 is incompatible with Firebird type 510"

Parameters are bound exactly; strings are not parsed into dates or numbers.
Pass the Python type that matches the SQL type:

| SQL type (code in the message) | Python value |
| --- | --- |
| `VARCHAR`/`CHAR` (448/452) | `str` |
| `SMALLINT`/`INTEGER`/`BIGINT`/`INT128` (500/496/580/32752) | `int` |
| `NUMERIC`/`DECIMAL` (stored as 500/496/580/32752 with a scale) | `decimal.Decimal` or `int` (a `float` is rejected) |
| `DOUBLE PRECISION`/`FLOAT` (480/482) | `float` (an `int` is rejected) |
| `DATE` (570) | `datetime.date` |
| `TIMESTAMP` (510) | `datetime.datetime` |
| `TIME` (560) | `datetime.time` |
| `BOOLEAN` (32764) | `bool` |

"Expected 1 parameters, received 0" means the number of `?` placeholders and
values differ. `params` must be a list, a tuple or `None`; one value is `[value]`.

### `DataConversionError`: "Duplicate column names; provide unique aliases"

Tuples allow repeated names, but dictionaries (`as_dict=True`), Pydantic models,
JSON with `orient="records"`, Arrow and Polars use column names as keys.
Give every column a unique alias: `SELECT O.ID, C.ID AS CUSTOMER_ID ...`.

### Pydantic: "Field required" for a column that is there

Firebird returns unquoted identifiers in UPPER CASE, so `SELECT id FROM ...`
produces the key `ID`. Name the fields in upper case, add
`Field(validation_alias="ID")`, or quote the alias in SQL (`AS "id"`). fbfast
does not rename columns.

### `UnsupportedTypeError`: "Polars cannot preserve Decimal256"

INT128-based values become Arrow Decimal256, and Polars cannot hold Decimal256.
In Firebird 4+ these appear more often than expected: `INT128` and
`NUMERIC(38, x)` columns, and also `SUM()` over `BIGINT` or `NUMERIC(18, x)`, and
`BIGINT * BIGINT`. Choose one:

- `CAST(SUM(TOTAL) AS NUMERIC(18, 2))` in SQL, when the value fits;
- `fetch_polars(..., unsupported="string")` to receive exact decimal text;
- `fetch_arrow(...)` (PyArrow keeps Decimal256), or `fetch_all` (Python `Decimal`).

## Slow queries over a WAN or tunnel

Start with `FirebirdConfig(diagnostics=True)`: each record of
`pool.diagnostics()` lists phases (`queue`, `begin`, `prepare`, `execute`,
`fetch`, `cleanup`...) with `seconds` and `roundtrips`. Round trips times your
latency is usually the whole story on a remote link.

- A cached query in a pool answers after 1 round trip (execute plus the first
  batch) when its connection has a transaction ready, and after 2 (begin,
  execute) otherwise. The session reset runs after the reply and starts that
  ready transaction; it is rolled back after `ready_transaction_timeout` (2 s)
  without use. `diagnostics` shows `begin` with 0 round trips when it was used.
- `DatabaseConnectionError` right after a network blip: if the link drops within
  `ready_transaction_timeout` of the previous query, the next query on that
  connection is the first exchange and may already have reached the server, so
  fbfast reports it instead of retrying. After that window, a dead connection is
  replaced before any SQL is sent. Set `ready_transaction_timeout=None` to always
  get the transparent replacement at the cost of one extra round trip.
- Many small queries in a row: put them in `pool.snapshot()`, about 1 round trip
  each, or run independent ones together with `run_all`.
- An idle connection is pinged after `health_check_after` seconds in the
  background, which also keeps tunnels and NAT mappings open.

### BLOB columns are slow

Firebird 5.0.3+ sends BLOBs up to `max_inline_blob_size` (65535 bytes by
default) inside the row. Without that, every BLOB value costs extra round trips.
fbfast 0.2.1 lost the inline path for text BLOBs; at 20 ms of latency, 1,000 rows
with a small text BLOB took 24 s with 0.2.1 and 0.19 s with 0.3.0.

1. Check `session_info()["inline_blobs"]`. It is `False` before server 5.0.3
   (protocol 19) or with `max_inline_blob_size=0`. Upgrade the server; prefer
   5.0.4 or later, because 5.0.3 keeps memory for each inline BLOB until the
   transaction ends.
2. UTF8, ISO8859_1, NONE, OCTETS and binary BLOBs are read inline. Text BLOBs in
   other charsets (WIN1252, for example) still go through the server's
   conversion filter, one extra round trip per value. Convert those columns, or
   `CAST` them in SQL.
3. Values larger than the inline limit are always read separately.
4. `CAST` short text to the smallest `VARCHAR` that fits, never `VARCHAR(8191)`
   by habit: wide rows mean fewer rows per round trip. `LIST()` returns a BLOB.
5. Compare `roundtrips` in `diagnostics()` with and without the BLOB column.

### Wire compression is not negotiated

`wire_compression=True` only asks; check `session_info()["wire_compressed"]`.
Both sides need zlib: Linux wheels use the system `libz.so.1` (install zlib if
it is missing), Windows wheels bundle `zlib1.dll`, and macOS wheels bundle a
small shim that loads the system zlib (the official macOS client looked for a
library name no macOS provides, so compression never worked there before 0.3.0).
If the client side is fine, the server lacks zlib. Compression helps on slow or
metered links and costs CPU on a fast LAN.

## Pools, snapshots and concurrency

### `PoolBusyError` versus `ConcurrentUseError`

| | `PoolBusyError` | `ConcurrentUseError` (a subclass) |
| --- | --- | --- |
| Cause | Too many callers: the waiting queue (`max_waiters`) is full, or more than `pool_size + max_waiters` async calls are pending. | One snapshot or stream used by two operations at once, e.g. a query while a stream from the same snapshot is open. |
| Fix | Back off and retry, raise `max_waiters`, or use `run_all`/`run_all_async` for batches, which never overflow the queue. | Serialize the use. Finish or close the stream first; never share a snapshot between threads or tasks. Retrying does not help. |

Catch `ConcurrentUseError` before `PoolBusyError` if you handle both.

### `PoolTimeoutError` while acquiring

No connection became ready within `acquire_timeout` (5 s). Either all
`pool_size` connections were busy (`stats()["leased"]`, `stats()["waiting"]`)
or opening a new one was slow or failing. Raise `pool_size`, shorten long
queries, or check the network.

### Which timeout fired?

| Setting | Default | Covers | Error |
| --- | --- | --- | --- |
| `acquire_timeout` | 5 s | Waiting for a ready connection, including opening one. | `PoolTimeoutError`, phase `acquire` |
| `connect_timeout` | 5 s | Attach and handshake of one connection (also bounded by `acquire_timeout`). | `PoolTimeoutError`, phase `attach` |
| `query_timeout` / `timeout=` | 30 s | One query, including fetching, conversion and consuming a stream. `None` disables it. | `QueryTimeoutError`, phase `query` or `fetch` |
| `snapshot_timeout` / `snapshot(timeout=)` | 300 s | The whole snapshot scope, including your code between queries. | `QueryTimeoutError`, phase `snapshot` |
| `run_all(timeout=)` | none | A whole query group; unfinished queries are cancelled. | `QueryTimeoutError`, phase `group` |
| `stream_idle_timeout` | 30 s | A stream waiting for its consumer to ask for more. | `QueryTimeoutError`, phase `stream_idle` |
| `close_timeout` | 10 s | Closing the pool, a snapshot or a stream. | `PoolTimeoutError` |
| `lock_timeout` | 5 s | Server-side lock waits. | `QueryError` |

Inside a snapshot each query uses the smaller of its own deadline and what is
left of the snapshot's. Passing `timeout=None` to a query does not disable the
snapshot deadline.

### Errors inside a snapshot

Any error ends a snapshot. Later calls on it, and leaving its `with` block,
raise that error again; catch it outside the block and open a new snapshot.
Using a snapshot or stream outside its `with` block raises `UsageError`.

### `TypeError: 'NoneType' object can't be awaited` after upgrading

0.3.0 renamed the lifecycle methods: no suffix blocks, `_async` (and `aclose`)
is a coroutine.

| Up to 0.2.x | 0.3.0 |
| --- | --- |
| `pool.close_sync()` / `await pool.close()` | `pool.close()` / `await pool.aclose()` |
| `pool.warmup_sync()` / `await pool.warmup()` | `pool.warmup()` / `await pool.warmup_async()` |
| `pool.clear_statement_cache_sync()` / `await pool.clear_statement_cache()` | `pool.clear_statement_cache()` / `await pool.clear_statement_cache_async()` |
| `snapshot.close_sync()` / `await snapshot.close()` | `snapshot.close()` / `await snapshot.aclose()` |
| `init_pool_sync()` / `await init_pool()` | `init_pool()` / `await init_pool_async()` |
| `close_pool_sync()` / `await close_pool()` | `close_pool()` / `await close_pool_async()` |
| `async with pool.snapshot_async()` | `async with pool.snapshot()` |

The old `_sync` names and `snapshot_async()` still work with a
`DeprecationWarning` until 0.4.0. Names whose meaning flipped have no alias:
`await pool.close()` now closes the pool synchronously and then fails with
`TypeError`, so old code fails loudly instead of leaking connections.

### `ForkSafetyError` and worker processes

Pools, snapshots and streams belong to the process that created them. Using
one in a child after `fork()` raises `ForkSafetyError`, and named pools
registered before the fork are rejected the same way.

- Importing fbfast before forking is fine; create pools in each worker after it
  starts (FastAPI lifespan, Gunicorn `post_fork`, Celery `worker_process_init`).
- Do not open a pool at import time in an app loaded with `--preload`.
- If the parent process has already used fbfast, start workers with the
  `spawn` start method (`multiprocessing.get_context("spawn")`). A child forked
  from such a process should not create a new pool either.

### Statement errors after a schema change

Prepared statements are cached per connection. After DDL changes a table,
drain active work and call `pool.clear_statement_cache()` (or
`await pool.clear_statement_cache_async()`), or recreate the pool.
