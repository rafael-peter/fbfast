from pathlib import Path
from typing import Any, Awaitable, Literal, final

def compiled_info() -> str: ...
def load_client(path: Path) -> dict[str, Any]: ...

@final
class ArrowSchema:
    """Owned schema exportable through the Arrow C Data protocol; no extra required."""
    @property
    def names(self) -> list[str]:
        """Column names in result order."""
    def __len__(self) -> int: ...
    def __arrow_c_schema__(self) -> object:
        """Arrow PyCapsule export of the schema (``pyarrow.schema(schema)``)."""

@final
class ArrowBatch:
    """Owned native columns; exported buffers remain valid after stream/pool close."""
    @property
    def num_rows(self) -> int:
        """Rows in this batch."""
    @property
    def num_columns(self) -> int:
        """Columns in this batch."""
    @property
    def nbytes(self) -> int:
        """Bytes held by the batch's Arrow buffers."""
    @property
    def schema(self) -> ArrowSchema:
        """Column names and Arrow types, including SQL metadata."""
    def __len__(self) -> int: ...
    def __arrow_c_array__(self, requested_schema: object | None = None) -> tuple[object, object]:
        """Arrow PyCapsule export, e.g. ``pyarrow.record_batch(batch)`` or ``polars.from_arrow``."""

@final
class _Stream:
    @property
    def schema(self) -> ArrowSchema: ...
    def __arrow_c_stream__(self, requested_schema: object | None = None) -> object: ...
    def open_sync(self) -> None: ...
    def next_sync(self) -> Any: ...
    def close_sync(self) -> None: ...
    def open_async(self) -> Awaitable[None]: ...
    def next_async(self) -> Awaitable[Any]: ...
    def close_async(self) -> Awaitable[None]: ...

@final
class _Pool:
    def arrow(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, mode: str = "arrow", timeout: float | None = None, asynchronous: bool = False) -> Any: ...
    def json(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, options: tuple[str, str, str, str, str], timeout: float | None = None, asynchronous: bool = False) -> Any: ...
    def stream_json(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, options: tuple[tuple[str, str, str, str, str], str], batch_rows: int | None = None, timeout: float | None = None) -> _Stream: ...
    def snapshot(self, *, timeout: float | None = None) -> _Pool: ...
    def request_close(self) -> None: ...
    def stream(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, batch_rows: int | None = None, as_dict: bool = False, timeout: float | None = None, arrow: bool = False) -> _Stream: ...
    def clear_statement_cache_sync(self, *, timeout: float | None = None) -> None: ...
    def clear_statement_cache(self, *, timeout: float | None = None) -> Awaitable[None]: ...
    def rows_async(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, first: bool = False, as_dict: bool = False, timeout: float | None = None) -> Awaitable[Any]: ...
    def value_async(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, timeout: float | None = None) -> Awaitable[Any]: ...
    def explain_async(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, timeout: float | None = None) -> Awaitable[str]: ...
    def warmup(self) -> Awaitable[None]: ...
    def close(self) -> Awaitable[None]: ...
    def __init__(self, config: FirebirdConfig, client_path: Path) -> None: ...
    def rows(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, first: bool = False, as_dict: bool = False, timeout: float | None = None) -> Any: ...
    def value(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, timeout: float | None = None) -> Any: ...
    def explain(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, timeout: float | None = None) -> str: ...
    def warmup_sync(self) -> None: ...
    def close_sync(self) -> None: ...
    def stats(self) -> dict[str, int | bool]: ...
    def diagnostics(self) -> list[dict[str, Any]]: ...
    def session_info(self) -> dict[str, Any] | None: ...
    def group(self, specs: list[tuple[str, str, list[Any] | tuple[Any, ...] | None, float | None, Any]], *, concurrency: int, timeout: float | None = None) -> _Group: ...

@final
class _Group:
    def next(self) -> tuple[int, bool, Any] | None: ...
    def cancel(self) -> None: ...
    def remaining(self) -> int: ...
    @property
    def concurrency(self) -> int: ...

@final
class FirebirdConfig:
    """Immutable connection and pool settings; ``FirebirdConfig()`` reads ``FB_*`` and ``.env``.

    Precedence per option: explicit argument > environment > ``.env`` file > default.
    ``database``, ``user`` and ``password`` have no default and must come from one
    of these sources. Construction validates everything but never connects.
    See the constructor parameters for meaning, limits and defaults.
    """

    # One logical line per parameter keeps its full help visible in Pyright/Pylance.
    def __init__(
        self,
        *,
        host: str = ...,
        port: int = ...,
        database: str = ...,
        user: str = ...,
        password: str = ...,
        role: str | None = ...,
        read_only: Literal[True] = ...,
        charset: str = ...,
        session_time_zone: str = ...,
        client_library: str | None = ...,
        wire_crypt: Literal["required", "enabled", "disabled"] | bool = ...,
        wire_compression: bool = ...,
        app_name: str = ...,
        pool_size: int = ...,
        min_idle: int = ...,
        max_waiters: int = ...,
        acquire_timeout: float = ...,
        connect_timeout: float = ...,
        idle_timeout: float = ...,
        max_lifetime: float = ...,
        health_check_after: float = ...,
        close_timeout: float = ...,
        fetch_size: int = ...,
        batch_bytes: int = ...,
        prefetch_batches: int = ...,
        max_result_bytes: int | None = ...,
        max_result_rows: int | None = ...,
        max_blob_bytes: int = ...,
        query_timeout: float | None = ...,
        lock_timeout: int = ...,
        stream_idle_timeout: float | None = ...,
        statement_cache_size: int = ...,
        statement_cache_bytes: int = ...,
        trim_char: bool = ...,
        fallback_charset: str | None = ...,
        snapshot_timeout: float | None = ...,
        max_inline_blob_size: int | None = ...,
        blob_cache_size: int | None = ...,
        diagnostics: bool = ...,
        ready_transaction_timeout: float | None = ...,
        env_file: str | Literal[False] = ...,
    ) -> None:
        """Resolve and validate the configuration without connecting.

        Each omitted argument uses the FB_<UPPER_NAME> environment variable, then
        the same variable in .env, then the default listed below. ``...`` in the
        signature means "omitted"; never pass Ellipsis as a value. An explicit
        None wins over the environment for options that accept None. Invalid
        values raise instead of silently falling back to a default. Durations
        are positive, finite seconds up to 4_294_967.295; lock_timeout accepts
        zero. In the environment/.env, numbers are text, booleans accept
        true/false, 1/0 and yes/no, and "none" means None. Reading .env never
        modifies os.environ.

        Args:
            host: Firebird server host name or address (non-empty str). [env FB_HOST; default "localhost"]
            port: TCP port (int, 1..65535). [env FB_PORT; default 3050]
            database: Database alias or path on the server (non-empty str). [env FB_DATABASE; required]
            user: Firebird user (non-empty str). [env FB_USER; required]
            password: Password (non-empty str), hidden in repr. Accessing config.password returns a normal str; never log it. [env FB_PASSWORD; required]
            role: SQL role to activate (str | None). An explicit None ignores a role from the environment. [env FB_ROLE; default None]
            read_only: Only True is supported (Literal[True]); False raises UnsupportedFeatureError. [env FB_READ_ONLY; default True]
            charset: Connection charset (non-empty str). Only UTF8 is supported by the executor. [env FB_CHARSET; default "UTF8"]
            session_time_zone: Session time zone such as "America/Sao_Paulo" (non-empty str). [env FB_SESSION_TIME_ZONE; default "UTC"]
            client_library: Path to a libfbclient to use instead of the bundled one (str | None). [env FB_CLIENT_LIBRARY; default None]
            wire_crypt: Wire encryption policy: "required", "enabled" or "disabled"; bools are accepted (True=required, False=disabled). "enabled" allows an unencrypted connection; keep "required" unless the transport is reviewed. [env FB_WIRE_CRYPT; default "required"]
            wire_compression: Request zlib wire compression; the server must agree. Helps on slow links, costs CPU on fast ones. Check Pool.session_info()["wire_compressed"]. [env FB_WIRE_COMPRESSION; default False]
            app_name: Application name reported to the server (non-empty str). [env FB_APP_NAME; default "fbfast"]
            pool_size: Maximum connections and native workers (int >= 1); must be >= min_idle. Also the default and maximum concurrency of run_all/run_many. [env FB_POOL_SIZE; default 8]
            min_idle: Connections kept open and ready (0 <= int <= pool_size). Use 0 to keep none. [env FB_MIN_IDLE; default 2]
            max_waiters: Calls allowed to wait in the queue (int >= 1); a full queue raises PoolBusyError. [env FB_MAX_WAITERS; default 256]
            acquire_timeout: Seconds to obtain a ready connection, including any attach/handshake (float; None not allowed). [env FB_ACQUIRE_TIMEOUT; default 5.0]
            connect_timeout: Seconds for attach and handshake, also bounded by acquire_timeout (float). [env FB_CONNECT_TIMEOUT; default 5.0]
            idle_timeout: Seconds idle before connections beyond min_idle are closed (float). [env FB_IDLE_TIMEOUT; default 300.0]
            max_lifetime: Target connection lifetime in seconds, jittered so recycling spreads out; active work is never interrupted (float). [env FB_MAX_LIFETIME; default 1800.0]
            health_check_after: Seconds idle after which a connection is pinged; done off the critical path, which also keeps tunnels and NAT mappings alive (float). [env FB_HEALTH_CHECK_AFTER; default 30.0]
            close_timeout: Seconds to wait for close/cleanup (float). Native work still pending after it stays counted until it finishes. [env FB_CLOSE_TIMEOUT; default 10.0]
            fetch_size: Target rows per batch (int >= 1); also the default batch_rows of streams. [env FB_FETCH_SIZE; default 5000]
            batch_bytes: Byte budget per batch (int >= 1); a single larger row forms its own batch. BLOB batches close once their decoded bytes reach it. Not a limit on total Python memory. [env FB_BATCH_BYTES; default 4_194_304 (4 MiB)]
            prefetch_batches: Batches fetched ahead of the consumer (int >= 0); zero disables read-ahead. [env FB_PREFETCH_BATCHES; default 1]
            max_result_bytes: Cumulative budget per materialized result (int >= 1 | None); does not apply to streams and is not an exact RSS cap. None disables it. [env FB_MAX_RESULT_BYTES; default 536_870_912 (512 MiB)]
            max_result_rows: Row limit per materialized result (int >= 1 | None); does not apply to streams. [env FB_MAX_RESULT_ROWS; default None]
            max_blob_bytes: Limit per BLOB value, also in streams (int >= 1). Known text charsets count bytes after conversion to UTF-8; exceeding it raises ResultLimitError. [env FB_MAX_BLOB_BYTES; default 67_108_864 (64 MiB)]
            query_timeout: Default per-query deadline in seconds, including fetching and Python conversion (float | None). None disables it. [env FB_QUERY_TIMEOUT; default 30.0]
            lock_timeout: Seconds the server waits for locks (int >= 0); zero means NO WAIT. [env FB_LOCK_TIMEOUT; default 5]
            stream_idle_timeout: Seconds a stream producer waits for the consumer to ask for more (float | None); None disables it. Independent of query_timeout. [env FB_STREAM_IDLE_TIMEOUT; default 30.0]
            statement_cache_size: Prepared statements kept per connection (int >= 0); zero disables the cache. [env FB_STATEMENT_CACHE_SIZE; default 64]
            statement_cache_bytes: Estimated statement cache budget per connection (int >= 1, bytes). [env FB_STATEMENT_CACHE_BYTES; default 8_388_608 (8 MiB)]
            trim_char: Strip trailing spaces from CHAR columns, never VARCHAR (bool). [env FB_TRIM_CHAR; default False]
            fallback_charset: Charset for text without one (charset NONE), str | None. Currently only UTF8/UTF-8; applies to text and NONE text BLOBs. Invalid bytes raise DataConversionError. [env FB_FALLBACK_CHARSET; default None]
            snapshot_timeout: Deadline for a whole snapshot scope, including time between its queries (float | None, seconds). Each query inside still uses query_timeout. None disables it. [env FB_SNAPSHOT_TIMEOUT; default 300.0]
            max_inline_blob_size: Largest BLOB a Firebird 5.0.3+ server sends together with its row (int 0..65535 | None, bytes); 0 disables inline BLOBs. None keeps the client default (65535). [env FB_MAX_INLINE_BLOB_SIZE; default None]
            blob_cache_size: Client cache for inline BLOBs per connection (int >= 0 | None, bytes). None keeps the client default (10 MiB). [env FB_BLOB_CACHE_SIZE; default None]
            diagnostics: Record time and round trips per phase for every pool query, read with Pool.diagnostics() (bool). Reading the counters adds no round trip. [env FB_DIAGNOSTICS; default False]
            ready_transaction_timeout: After a query that read all its rows, its session reset also starts the next query's transaction, so the next query skips one round trip; an unused transaction is rolled back after this many idle seconds (float | None). None disables the reuse. [env FB_READY_TRANSACTION_TIMEOUT; default 2.0]
            env_file: dotenv path (str) or False to skip it. Omitted: read .env in the current directory if it exists; an explicit path must exist. False keeps reading FB_* variables. There is no FB_ENV_FILE; True and None are rejected.

        Raises:
            ConfigurationError: A required option is missing, a value is invalid,
                an option is unknown or the dotenv file is invalid.
            UnsupportedFeatureError: read_only=False.
        """
        ...
    @classmethod
    def from_env(cls, prefix: str = "FB_", *, env_file: str | Literal[False] = ...) -> FirebirdConfig:
        """Optional shortcut for a custom variable prefix; FirebirdConfig() already reads FB_*.

        Args:
            prefix: Variable prefix; default "FB_". Example: "FB_BI_" reads FB_BI_HOST...
            env_file: dotenv path or False; omitted reads .env in the current directory.
        """
        ...
    @property
    def host(self) -> str: ...
    @property
    def port(self) -> int: ...
    @property
    def database(self) -> str: ...
    @property
    def user(self) -> str: ...
    @property
    def password(self) -> str: ...
    @property
    def role(self) -> str | None: ...
    @property
    def read_only(self) -> Literal[True]: ...
    @property
    def charset(self) -> str: ...
    @property
    def session_time_zone(self) -> str: ...
    @property
    def client_library(self) -> str | None: ...
    @property
    def wire_crypt(self) -> Literal["required", "enabled", "disabled"]: ...
    @property
    def wire_compression(self) -> bool: ...
    @property
    def app_name(self) -> str: ...
    @property
    def pool_size(self) -> int: ...
    @property
    def min_idle(self) -> int: ...
    @property
    def max_waiters(self) -> int: ...
    @property
    def acquire_timeout(self) -> float: ...
    @property
    def connect_timeout(self) -> float: ...
    @property
    def idle_timeout(self) -> float: ...
    @property
    def max_lifetime(self) -> float: ...
    @property
    def health_check_after(self) -> float: ...
    @property
    def close_timeout(self) -> float: ...
    @property
    def fetch_size(self) -> int: ...
    @property
    def batch_bytes(self) -> int: ...
    @property
    def prefetch_batches(self) -> int: ...
    @property
    def max_result_bytes(self) -> int | None: ...
    @property
    def max_result_rows(self) -> int | None: ...
    @property
    def max_blob_bytes(self) -> int: ...
    @property
    def query_timeout(self) -> float | None: ...
    @property
    def lock_timeout(self) -> int: ...
    @property
    def stream_idle_timeout(self) -> float | None: ...
    @property
    def statement_cache_size(self) -> int: ...
    @property
    def statement_cache_bytes(self) -> int: ...
    @property
    def trim_char(self) -> bool: ...
    @property
    def fallback_charset(self) -> str | None: ...
    @property
    def snapshot_timeout(self) -> float | None: ...
    @property
    def max_inline_blob_size(self) -> int | None: ...
    @property
    def blob_cache_size(self) -> int | None: ...
    @property
    def diagnostics(self) -> bool: ...
    @property
    def ready_transaction_timeout(self) -> float | None: ...
