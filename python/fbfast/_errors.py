"""Exception hierarchy shared by the native core and the Python layer.

Defined in Python (not in the extension) so classes can also inherit from the
built-in ``TimeoutError``, ``ConnectionError``, ``RuntimeError`` and
``ValueError``. The native core creates instances by class name. This module
must not import ``fbfast._native``.
"""
from __future__ import annotations

from typing import TypeVar

__all__ = [
    "ArgumentError", "AuthenticationError", "CleanupError", "ClientLibraryError",
    "ConcurrentUseError", "ConfigurationError", "DataConversionError",
    "DatabaseConnectionError", "FbFastError", "ForkSafetyError", "PoolAlreadyInitializedError",
    "PoolBusyError", "PoolClosedError", "PoolError", "PoolNotInitializedError",
    "PoolTimeoutError", "QueryCancelledError", "QueryError", "QueryTimeoutError",
    "ReadOnlyViolationError", "ResultLimitError", "RoleNotActiveError", "UnsupportedFeatureError",
    "UnsupportedServerError", "UnsupportedStatementError", "UnsupportedTypeError", "UsageError",
]


class FbFastError(Exception):
    """Base class of every error raised by fbfast.

    Attributes:
        phase: Where it failed, e.g. ``"attach"``, ``"prepare"``, ``"fetch"``,
            ``"acquire"``, ``"cleanup"`` or ``"python"`` for checks made in Python.
        sqlstate: Five-character SQLSTATE reported by Firebird, or ``None``.
        native_codes: Firebird status vector codes (``isc_*`` numbers).
        cleanup_errors: Secondary failures raised while cleaning up after this one.
    """

    phase: str
    sqlstate: str | None
    native_codes: list[int]
    cleanup_errors: list[BaseException]

    def __init__(self, *args: object, phase: str = "python", sqlstate: str | None = None,
                 native_codes: list[int] | None = None,
                 cleanup_errors: list[BaseException] | None = None) -> None:
        super().__init__(*args)
        self.phase = phase
        self.sqlstate = sqlstate
        self.native_codes = list(native_codes or ())
        self.cleanup_errors = list(cleanup_errors or ())


class ConfigurationError(FbFastError):
    """Invalid, missing or unknown configuration value. Fix the configuration; do not retry."""


class UnsupportedFeatureError(FbFastError):
    """The requested option or operation is outside what fbfast supports (e.g. ``read_only=False``)."""


class UnsupportedStatementError(FbFastError):
    """The SQL is not a single read-only SELECT/WITH query (DML, DDL, EXECUTE BLOCK...)."""


class ReadOnlyViolationError(FbFastError):
    """The server reported or the handshake detected a write attempt or a non read-only transaction."""


class ClientLibraryError(FbFastError):
    """The Firebird client library could not be loaded or is not a supported 5.0.3+ client."""


class UsageError(FbFastError, RuntimeError):
    """API misuse detected in Python, such as querying a snapshot or stream outside its ``with`` block."""


class ArgumentError(FbFastError, ValueError, TypeError):
    """Invalid argument value or type passed to an fbfast function."""


class PoolError(FbFastError):
    """Base class of pool lifecycle and capacity errors."""


class ForkSafetyError(PoolError):
    """A pool, stream or snapshot created before ``fork()`` was used in the child. Create pools after forking."""


class PoolClosedError(PoolError):
    """The pool is closing or closed, or its worker stopped. Create a new pool to continue."""


class PoolBusyError(PoolError):
    """The waiting queue is full (``max_waiters``). Retrying later can succeed."""


class ConcurrentUseError(PoolBusyError):
    """A snapshot or stream was used by two operations at once. Serialize its use; retrying will not help."""


class PoolTimeoutError(PoolError, TimeoutError):
    """No connection became available within ``acquire_timeout``. Retrying later can succeed."""


class PoolNotInitializedError(PoolError):
    """No ready pool is registered under the requested name."""


class PoolAlreadyInitializedError(PoolError):
    """A name is already reserved, including while opening or draining."""


class QueryCancelledError(FbFastError):
    """The operation was cancelled (task cancellation, stream close or group cancellation)."""


class QueryError(FbFastError):
    """Firebird rejected or failed the statement. Inspect ``sqlstate`` and ``native_codes``."""


class QueryTimeoutError(QueryError, TimeoutError):
    """The query deadline (``query_timeout``, ``timeout=`` or the snapshot deadline) expired."""


class DatabaseConnectionError(QueryError, ConnectionError):
    """The server or network refused the connection or dropped it (SQLSTATE 08xxx).

    fbfast never repeats SQL that may have reached the server. Retrying the
    whole operation is safe when it is idempotent, as plain SELECTs are.
    """


class AuthenticationError(FbFastError):
    """The server rejected the user name or password (SQLSTATE 28000)."""


class RoleNotActiveError(FbFastError):
    """The requested role is not active for the session (missing grant or wrong name)."""


class UnsupportedServerError(FbFastError):
    """The server is not Firebird 5.0.x."""


class DataConversionError(FbFastError):
    """A value could not be converted exactly between Firebird and Python."""


class UnsupportedTypeError(FbFastError):
    """The result or a parameter uses a type fbfast does not convert (e.g. ARRAY)."""


class ResultLimitError(FbFastError):
    """A materialized result or BLOB exceeded ``max_result_bytes``, ``max_result_rows`` or ``max_blob_bytes``."""


class CleanupError(FbFastError):
    """Closing a cursor, transaction, statement or connection failed; the connection was discarded."""


_Error = TypeVar("_Error", bound=FbFastError)


def usage(kind: type[_Error], message: str, *, phase: str = "python") -> _Error:
    """Creates a Python-side error with the same attributes native errors carry."""
    return kind(message, phase=phase)


def add_cleanup_note(original: BaseException | None, cleanup: Exception, scope: str) -> None:
    """Attaches a secondary cleanup failure to the original error, or raises it."""
    if original is None:
        raise cleanup
    original.add_note(f"fbfast {scope} cleanup also failed: {cleanup}")
    if isinstance(original, FbFastError):
        original.cleanup_errors.append(cleanup)


# Public names live in the ``fbfast`` namespace (tracebacks, pickling and repr).
for _name in __all__:
    globals()[_name].__module__ = "fbfast"
del _name
