"""Read-only Firebird 5 client with a Rust core. Importing never connects or downloads.

Configure::

    config = fbfast.FirebirdConfig()          # reads FB_* variables and .env
    with fbfast.Pool(config) as pool:          # or: async with fbfast.Pool(config) as pool
        rows = pool.fetch_all("SELECT ID, NAME FROM CUSTOMERS WHERE CITY = ?", ["Recife"])

Naming rule: methods without a suffix block; ``*_async`` methods (and ``aclose``)
are coroutines. Parameters are positional ``?`` placeholders.

Goal                              Method (Pool or Snapshot)
--------------------------------  -----------------------------------------------
All rows as tuples/dicts/models   fetch_all(sql, params, as_dict=, model=)
First row or None                 fetch_one(...)
First column of the first row     fetch_val(...)
JSON bytes built natively         fetch_json(...)
pyarrow.Table / polars.DataFrame  fetch_arrow(...) / fetch_polars(...)
Large results in batches          with stream(...) / stream_json(...) / stream_arrow(...)
Several queries at once           run_all([...]) / run_many([...]) with Query objects
Consistent multi-query reads      with pool.snapshot() as snap: snap.fetch_all(...)
Execution plan                    explain(...)
Counters / per-query timings      stats() / diagnostics() / session_info()

Named pools: ``init_pool(config, name=...)`` registers one, ``get_pool(name)``
returns it anywhere in the process, and ``close_pool(name)`` closes it. The module
functions (``fbfast.fetch_all(sql, pool="default")``...) are shortcuts for
``get_pool(pool).fetch_all(...)``.

Errors derive from :class:`FbFastError`; timeouts are also ``TimeoutError`` and
lost connections also ``ConnectionError``. fbfast only reads, but it is not a
sandbox: give it a role limited to the tables it needs. ``llms.txt`` in this
package summarizes the rules for code assistants.
"""

from ._bootstrap import build_info, client_info
from ._errors import (
    ArgumentError,
    AuthenticationError,
    CleanupError,
    ClientLibraryError,
    ConcurrentUseError,
    ConfigurationError,
    DatabaseConnectionError,
    DataConversionError,
    FbFastError,
    ForkSafetyError,
    PoolAlreadyInitializedError,
    PoolBusyError,
    PoolClosedError,
    PoolError,
    PoolNotInitializedError,
    PoolTimeoutError,
    QueryCancelledError,
    QueryError,
    QueryTimeoutError,
    ReadOnlyViolationError,
    ResultLimitError,
    RoleNotActiveError,
    UnsupportedFeatureError,
    UnsupportedServerError,
    UnsupportedStatementError,
    UnsupportedTypeError,
    UsageError,
)
from ._native import ArrowBatch, ArrowSchema, FirebirdConfig
from ._types import (
    DEFAULT,
    DefaultTimeout,
    Params,
    PhaseTrace,
    PoolStats,
    QueryTrace,
    SessionInfo,
    Timeout,
)
from .api import (
    explain,
    explain_async,
    fetch_all,
    fetch_all_async,
    fetch_arrow,
    fetch_arrow_async,
    fetch_json,
    fetch_json_async,
    fetch_one,
    fetch_one_async,
    fetch_polars,
    fetch_polars_async,
    fetch_val,
    fetch_val_async,
    stream,
    stream_arrow,
    stream_arrow_async,
    stream_async,
    stream_json,
    stream_json_async,
)
from .group import AsyncQueryGroup, QueryGroup
from .pool import Pool
from .query import Done, Query, Results
from .registry import (
    close_pool,
    close_pool_async,
    close_pool_sync,
    get_pool,
    init_pool,
    init_pool_async,
    init_pool_sync,
)
from .snapshot import Snapshot
from .stream import ArrowStream, AsyncArrowStream, AsyncJsonStream, AsyncStream, JsonStream, Stream
from .temporal import TimeWithZone

__version__ = "0.3.0"
__all__ = [
    "DEFAULT",
    "ArgumentError",
    "ArrowBatch",
    "ArrowSchema",
    "ArrowStream",
    "AsyncArrowStream",
    "AsyncJsonStream",
    "AsyncQueryGroup",
    "AsyncStream",
    "AuthenticationError",
    "CleanupError",
    "ClientLibraryError",
    "ConcurrentUseError",
    "ConfigurationError",
    "DataConversionError",
    "DatabaseConnectionError",
    "DefaultTimeout",
    "Done",
    "FbFastError",
    "FirebirdConfig",
    "ForkSafetyError",
    "JsonStream",
    "Params",
    "PhaseTrace",
    "Pool",
    "PoolAlreadyInitializedError",
    "PoolBusyError",
    "PoolClosedError",
    "PoolError",
    "PoolNotInitializedError",
    "PoolStats",
    "PoolTimeoutError",
    "Query",
    "QueryCancelledError",
    "QueryError",
    "QueryGroup",
    "QueryTimeoutError",
    "QueryTrace",
    "ReadOnlyViolationError",
    "ResultLimitError",
    "Results",
    "RoleNotActiveError",
    "SessionInfo",
    "Snapshot",
    "Stream",
    "TimeWithZone",
    "Timeout",
    "UnsupportedFeatureError",
    "UnsupportedServerError",
    "UnsupportedStatementError",
    "UnsupportedTypeError",
    "UsageError",
    "__version__",
    "build_info",
    "client_info",
    "close_pool",
    "close_pool_async",
    "close_pool_sync",
    "explain",
    "explain_async",
    "fetch_all",
    "fetch_all_async",
    "fetch_arrow",
    "fetch_arrow_async",
    "fetch_json",
    "fetch_json_async",
    "fetch_one",
    "fetch_one_async",
    "fetch_polars",
    "fetch_polars_async",
    "fetch_val",
    "fetch_val_async",
    "get_pool",
    "init_pool",
    "init_pool_async",
    "init_pool_sync",
    "stream",
    "stream_arrow",
    "stream_arrow_async",
    "stream_async",
    "stream_json",
    "stream_json_async",
]
