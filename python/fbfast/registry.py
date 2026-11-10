"""Process-local names for ready pools; execution stays in the native scheduler."""
from __future__ import annotations

import os
from _thread import LockType
from dataclasses import dataclass
from threading import Lock
from typing import Literal

from ._errors import (
    ConfigurationError, FbFastError, ForkSafetyError, PoolAlreadyInitializedError,
    PoolClosedError, PoolNotInitializedError, usage,
)
from ._native import FirebirdConfig
from ._types import deprecated
from .pool import Pool

__all__ = [
    "PoolAlreadyInitializedError", "PoolNotInitializedError", "close_pool", "close_pool_async",
    "close_pool_sync", "get_pool", "init_pool", "init_pool_async", "init_pool_sync",
]


@dataclass
class _Entry:
    pool: Pool
    state: Literal["opening", "ready", "closing"] = "opening"


_pid = os.getpid()
_lock = Lock()
_entries: dict[str, _Entry] = {}


def _error(kind: type[FbFastError], message: str) -> FbFastError:
    return usage(kind, message, phase="registry")


def _guard() -> LockType:
    global _pid, _lock
    pid = os.getpid()
    if pid != _pid:
        # Inspect PID before touching the inherited mutex or any pool. An import
        # before fork is supported; inheriting registered pools is not.
        if _entries:
            raise _error(ForkSafetyError, "Registered pools belong to another process; "
                         "initialize after fork or use spawn")
        _lock = Lock()
        _pid = pid
    return _lock


def _name(name: str) -> None:
    if not isinstance(name, str) or not name.strip():
        raise _error(ConfigurationError, "Pool name must be a nonempty string")


def _reserve(config: FirebirdConfig, name: str) -> _Entry:
    _name(name)
    with _guard():
        if name in _entries:
            raise _error(PoolAlreadyInitializedError, f"Pool {name!r} is already reserved")
        # Construction is lazy: no client loading, network, or worker creation.
        entry = _Entry(Pool(config))
        _entries[name] = entry
        return entry


def _publish(name: str, entry: _Entry) -> Pool:
    with _guard():
        if _entries.get(name) is not entry or entry.state != "opening":
            raise _error(PoolClosedError, f"Pool {name!r} was closed during initialization")
        entry.state = "ready"
        return entry.pool


def _remove(name: str, entry: _Entry) -> None:
    with _guard():
        # A delayed opener/closer must never remove a replacement instance.
        if _entries.get(name) is entry:
            del _entries[name]


def _closing(entry: _Entry) -> None:
    with _guard():
        entry.state = "closing"
    # Nonblocking native notification happens even if the next await is cancelled.
    entry.pool._core.request_close()


def _lookup_close(name: str) -> _Entry | None:
    _name(name)
    with _guard():
        entry = _entries.get(name)
        if entry is not None:
            entry.state = "closing"
    if entry is not None:
        entry.pool._core.request_close()
    return entry


def _cleanup_note(original: BaseException, cleanup: BaseException) -> None:
    original.add_note(f"fbfast registry cleanup also failed: {cleanup}")
    if isinstance(original, FbFastError) and isinstance(cleanup, FbFastError):
        original.cleanup_errors.append(cleanup)


def init_pool(config: FirebirdConfig, *, name: str = "default") -> Pool:
    """Create, warm and register a pool under ``name``; return it.

    The name is reserved while opening. On failure the pool is drained before
    the name is released; if that cleanup times out, call ``close_pool(name)``
    again (another initialization cannot replace it meanwhile).

    Raises:
        PoolAlreadyInitializedError: ``name`` is already registered or reserved.
    """
    entry = _reserve(config, name)
    try:
        entry.pool.warmup()
        return _publish(name, entry)
    except BaseException as original:
        try:
            _closing(entry)
            entry.pool.close()
        except BaseException as cleanup:
            _cleanup_note(original, cleanup)
        else:
            _remove(name, entry)
        raise


async def init_pool_async(config: FirebirdConfig, *, name: str = "default") -> Pool:
    """Async :func:`init_pool`, sharing the same registry and native pool.

    Failure or cancellation requests cleanup. If draining fails or is
    interrupted, the name stays reserved until ``close_pool_async(name)`` succeeds.
    """
    entry = _reserve(config, name)
    try:
        await entry.pool.warmup_async()
        return _publish(name, entry)
    except BaseException as original:
        try:
            _closing(entry)
            await entry.pool.aclose()
        except BaseException as cleanup:
            _cleanup_note(original, cleanup)
        else:
            _remove(name, entry)
        raise


def get_pool(name: str = "default") -> Pool:
    """Return the pool registered under ``name``; never opens one implicitly.

    This is how code that did not create the pool (a FastAPI route, a job,
    another module) reaches it. Module functions such as ``fbfast.fetch_all``
    call ``get_pool(pool).fetch_all(...)`` for you.

    Raises:
        PoolNotInitializedError: no ready pool has that name (also while opening or closing).
    """
    _name(name)
    with _guard():
        entry = _entries.get(name)
        if entry is None or entry.state != "ready":
            raise _error(PoolNotInitializedError, f"Pool {name!r} is not initialized")
        return entry.pool


def close_pool(name: str = "default") -> None:
    """Stop new lookups, drain and unregister the pool. An absent name is a no-op.

    A timeout or error keeps the name reserved; call again to finish draining.
    """
    entry = _lookup_close(name)
    if entry is not None:
        entry.pool.close()
        _remove(name, entry)


async def close_pool_async(name: str = "default") -> None:
    """Async :func:`close_pool`. Cancellation keeps the reservation for a retry."""
    entry = _lookup_close(name)
    if entry is not None:
        await entry.pool.aclose()
        _remove(name, entry)


def init_pool_sync(config: FirebirdConfig, *, name: str = "default") -> Pool:
    """Deprecated alias of :func:`init_pool`."""
    deprecated("init_pool_sync()", "init_pool()")
    return init_pool(config, name=name)


def close_pool_sync(name: str = "default") -> None:
    """Deprecated alias of :func:`close_pool`."""
    deprecated("close_pool_sync()", "close_pool()")
    close_pool(name)
