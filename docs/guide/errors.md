# Errors

Every error raised by fbfast derives from `fbfast.FbFastError`. The classes are
defined in [`python/fbfast/_errors.py`](../../python/fbfast/_errors.py)
and exported from the `fbfast` namespace. Some also derive from a built-in
exception, so generic handlers keep working:

- `QueryTimeoutError` and `PoolTimeoutError` are also `TimeoutError`.
- `DatabaseConnectionError` is also `ConnectionError`.
- `UsageError` is also `RuntimeError`; `ArgumentError` is also `ValueError` and `TypeError`.

Two errors that are **not** `FbFastError` can reach you from fbfast calls:
`pydantic.ValidationError` when a row does not fit your `model=` (rows are never
skipped silently), and `ImportError` when an optional extra (`pydantic`, `arrow`,
`polars`) is not installed.

## Hierarchy

```text
FbFastError
├── ConfigurationError
├── UnsupportedFeatureError
├── UnsupportedStatementError
├── ReadOnlyViolationError
├── ClientLibraryError
├── UsageError                 (also RuntimeError)
├── ArgumentError              (also ValueError, TypeError)
├── PoolError
│   ├── ForkSafetyError
│   ├── PoolClosedError
│   ├── PoolBusyError
│   │   └── ConcurrentUseError
│   ├── PoolTimeoutError       (also TimeoutError)
│   ├── PoolNotInitializedError
│   └── PoolAlreadyInitializedError
├── QueryCancelledError
├── QueryError
│   ├── QueryTimeoutError      (also TimeoutError)
│   └── DatabaseConnectionError (also ConnectionError)
├── AuthenticationError
├── RoleNotActiveError
├── UnsupportedServerError
├── DataConversionError
├── UnsupportedTypeError
├── ResultLimitError
└── CleanupError
```

## Reference

"Retry?" answers one question: can running the same call again, unchanged,
succeed? fbfast itself never repeats SQL that already reached the server.

| Exception | Base classes | Typical `phase` | When it happens | Retry? |
| --- | --- | --- | --- | --- |
| `ConfigurationError` | `FbFastError` | `config`, `registry` | Missing, invalid or unknown `FirebirdConfig` option, unreadable `.env`, empty pool name. | No. Fix the configuration. |
| `UnsupportedFeatureError` | `FbFastError` | `config` | `read_only=False`, or a connection charset other than UTF8. | No. |
| `UnsupportedStatementError` | `FbFastError` | `validate` | The SQL is not a single read-only `SELECT`/`WITH` query (DML, DDL, `EXECUTE BLOCK`, `WITH LOCK`, several statements). Nothing was sent to the server. | No. |
| `ReadOnlyViolationError` | `FbFastError` | `validate`, `attach` | A write was detected, or the session handshake did not confirm a READ ONLY transaction. | No. |
| `ClientLibraryError` | `FbFastError` | `load_client` | The Firebird client library could not be loaded or is older than 5.0.3. | No. Reinstall the wheel or fix `client_library`. |
| `UsageError` | `FbFastError`, `RuntimeError` | `python` | API misuse: querying a snapshot or stream outside its `with` block, entering one twice, `concurrency=0` in `run_all`. | No. Fix the code. |
| `ArgumentError` | `FbFastError`, `ValueError`, `TypeError` | `python` | Invalid argument: `model=` that is not a Pydantic v2 model, a non-`Query` in `run_all`, the same `Query` object twice, `unsupported=` other than `"error"`/`"string"`. | No. |
| `ForkSafetyError` | `PoolError` | `lifecycle`, `registry`, `async_runtime` | A pool, snapshot or stream created before `fork()` was used in the child. | No. Create pools after forking. |
| `PoolClosedError` | `PoolError` | `acquire`, `snapshot`, `stream`, `worker` | The pool is closing or closed; a snapshot already ended (any error ends it). | No. Use a new pool or a new snapshot. |
| `PoolBusyError` | `PoolError` | `acquire`, `delivery` | The waiting queue is full (`max_waiters`), or more async calls are pending than `pool_size + max_waiters`. | Yes, after a pause. For many queries use `run_all_async`. |
| `ConcurrentUseError` | `PoolBusyError` | `snapshot`, `stream` | A snapshot or stream was used by two operations at once (for example a query while a stream from the same snapshot is open). | No. Serialize the use. |
| `PoolTimeoutError` | `PoolError`, `TimeoutError` | `acquire`, `attach`, `close` | No connection became ready within `acquire_timeout` (or `connect_timeout` for opening one); closing did not finish within `close_timeout`. | Yes, later. Check pool size and the server. |
| `PoolNotInitializedError` | `PoolError` | `registry` | `get_pool(name)` found no ready pool (never initialized, still opening or closing). | No. Call `init_pool` first. |
| `PoolAlreadyInitializedError` | `PoolError` | `registry` | `init_pool(name=...)` with a name that is registered, opening or draining. | No. |
| `QueryCancelledError` | `FbFastError` | `query`, `stream`, `delivery` | The operation was cancelled: its stream was closed, its group was cancelled or the pool shut down. | Only if the cancellation was not intended. |
| `QueryError` | `FbFastError` | `prepare`, `execute`, `fetch`, ... | Firebird rejected or failed the statement (unknown column, missing privilege, arithmetic overflow...). Inspect `sqlstate` and `native_codes`. | Usually no. |
| `QueryTimeoutError` | `QueryError`, `TimeoutError` | `query`, `fetch`, `snapshot`, `group`, `stream_idle` | The deadline expired: `query_timeout` or `timeout=`, the snapshot deadline, a group `timeout=`, or `stream_idle_timeout`. | Only with a longer deadline or a cheaper query. |
| `DatabaseConnectionError` | `QueryError`, `ConnectionError` | `attach`, any (SQLSTATE `08xxx`) | The connection could not be opened or was lost. A failure before any SQL is sent is already retried once inside `acquire_timeout`. | Yes. A plain `SELECT` is idempotent; fbfast did not repeat it. |
| `AuthenticationError` | `FbFastError` | `attach` | SQLSTATE `28000`: wrong user or password, or incompatible wire encryption (`native_codes` contains `335545064`). | No. |
| `RoleNotActiveError` | `FbFastError` | `attach` | `role=` is not active for the session: wrong name or no `GRANT role TO user`. | No. |
| `UnsupportedServerError` | `FbFastError` | `attach` | The server is not Firebird 5.0.x. | No. |
| `DataConversionError` | `FbFastError` | `bind`, `describe`, `decode`, `python_conversion`, `polars` | A value cannot be converted exactly; wrong number or type of parameters; duplicate column names where names are keys (dicts, models, JSON records, Arrow); invalid bytes for `fallback_charset`. | No. |
| `UnsupportedTypeError` | `FbFastError` | `describe`, `decode` | A column or parameter type fbfast does not convert (ARRAY), or Decimal256 in `fetch_polars` without `unsupported="string"`. | No. |
| `ResultLimitError` | `FbFastError` | `materialize`, `read_blob` | A materialized result exceeded `max_result_bytes` or `max_result_rows`, or one BLOB exceeded `max_blob_bytes`. | No. Stream, filter, or raise the limit. |
| `CleanupError` | `FbFastError` | `cleanup`, `close` | Closing a cursor, transaction, statement or connection failed; the connection was discarded. | The pool keeps working; see below. |

`PoolError` is the base of the pool classes and is never raised by itself;
`FbFastError` is the catch-all.

## Reading an error

Every instance carries four attributes, also on errors raised in Python:

| Attribute | Type | Meaning |
| --- | --- | --- |
| `phase` | `str` | Where it failed. `"python"` for checks made in Python, `"config"` for configuration, `"attach"` for opening a connection, `"acquire"` for waiting on the pool, `"validate"` for the SQL checks done before anything is sent, `"prepare"`/`"bind"`/`"execute"`/`"fetch"` for the statement, `"query"`/`"snapshot"`/`"group"` for deadlines, `"cleanup"`/`"close"` for releasing resources. |
| `sqlstate` | `str \| None` | Five-character SQLSTATE reported by Firebird, such as `42S22` (unknown column), `28000` (authentication) or `08006` (connection failure). `None` when the error did not come from the server. |
| `native_codes` | `list[int]` | The Firebird status vector (`isc_*` codes), outermost first. Search them in the Firebird error-code list when the message is not enough. |
| `cleanup_errors` | `list[BaseException]` | Secondary failures that happened while cleaning up after this error. The original error is kept; each secondary failure is also added to the traceback with `add_note()`. |

```python
import fbfast

try:
    pool.fetch_all("SELECT NO_SUCH_COLUMN FROM RDB$DATABASE")
except fbfast.QueryError as error:
    print(type(error).__name__, error.phase, error.sqlstate, error.native_codes)
    # QueryError prepare 42S22 [335544569, 335544436, 335544578, 335544382, 336397208]
```

### Cleanup failures after the reply

Since 0.3.0 a pooled query returns as soon as its result is complete and its
cursor is closed. Rollback and `ALTER SESSION RESET` run afterwards, while the
connection stays unavailable to other callers ("deferred cleanup"). If that
cleanup fails, the connection is discarded and the failure is counted in
`pool.stats()["cleanup_errors"]`; the caller keeps its complete, valid result
and receives no `CleanupError`. `CleanupError` is still raised when cleanup is
part of the call you made: `pool.close()`, leaving a snapshot or a stream.

### Errors in query groups

With `run_all(..., fail_fast=True)` (the default) the first error cancels the
other queries and is raised from `run_all`. With `fail_fast=False`, every query
runs; `results.errors` maps each failed `Query` to its error and
`results[query]` raises it. In `run_many`, `done.result()` raises and
`done.exception()` returns the error. A group `timeout=` raises
`QueryTimeoutError` with `phase == "group"`.

## Retrying safely

Retry only errors that can succeed unchanged, and never retry
`ConcurrentUseError` even though it is a `PoolBusyError`:

```python
import time

import fbfast

RETRYABLE = (fbfast.DatabaseConnectionError, fbfast.PoolTimeoutError, fbfast.PoolBusyError)


def fetch_all_with_retry(pool: fbfast.Pool, sql: str, params=None, attempts: int = 3):
    for attempt in range(attempts):
        try:
            return pool.fetch_all(sql, params)
        except RETRYABLE as error:
            if isinstance(error, fbfast.ConcurrentUseError) or attempt == attempts - 1:
                raise
            time.sleep(0.2 * 2**attempt)
```

Rules of thumb:

- Before any SQL is sent, fbfast already replaces a broken idle connection once
  within `acquire_timeout` (counted in `stats()["reconnects"]`). After SQL was
  sent, the error is returned, never retried. Within `ready_transaction_timeout`
  (2 s) of a connection's previous query, the query itself is the first exchange,
  so a link that dropped meanwhile surfaces as `DatabaseConnectionError`.
- A `SELECT` from a selectable procedure can have side effects (generators are
  not rolled back). Retry those only if they are idempotent.
- Do not retry inside an open snapshot: any error ends it. Start a new one.
- When the session handshake finds a mismatch with your configuration
  (charset, time zone, wire-encryption policy), fbfast raises a plain
  `QueryError` with `phase == "attach"` and no `sqlstate`: retrying does not fix
  those. `DatabaseConnectionError` is reserved for failures the server or the
  network reported (SQLSTATE class 08, or a refused attach).

See [troubleshooting](troubleshooting.md) for specific messages and fixes.
