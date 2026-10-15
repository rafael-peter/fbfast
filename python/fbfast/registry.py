"""Process-local names for ready pools; execution stays in the native scheduler."""
from __future__ import annotations

import os
from _thread import LockType
from dataclasses import dataclass
from threading import Lock
from typing import Literal, TypeVar

from ._native import (
    ConfigurationError, FbFastError, FirebirdConfig, ForkSafetyError,
    PoolClosedError, PoolError,
)
from .pool import Pool


class PoolNotInitializedError(PoolError):
    """No ready pool is registered under the requested name."""


class PoolAlreadyInitializedError(PoolError):
    """A name is already reserved, including while opening or draining."""


@dataclass
class _Entry:
    pool: Pool
    state: Literal["opening", "ready", "closing"] = "opening"


_pid = os.getpid()
_lock = Lock()
_entries: dict[str, _Entry] = {}
_Error = TypeVar("_Error", bound=FbFastError)


def _error(kind: type[_Error], message: str) -> _Error:
    error = kind(message)
    error.phase = "registry"
    error.sqlstate = None
    error.native_codes = []
    error.cleanup_errors = []
    return error


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


def init_pool_sync(config: FirebirdConfig, *, name: str = "default") -> Pool:
    """Warm a pool before publishing it. A name is reserved throughout opening.

    On failure, drain before releasing the name. If cleanup times out, call
    close_pool_sync(name) again; another initialization cannot replace it.
    """
    entry = _reserve(config, name)
    try:
        entry.pool.warmup_sync()
        return _publish(name, entry)
    except BaseException as original:
        try:
            _closing(entry)
            entry.pool.close_sync()
        except BaseException as cleanup:
            _cleanup_note(original, cleanup)
        else:
            _remove(name, entry)
        raise


async def init_pool(config: FirebirdConfig, *, name: str = "default") -> Pool:
    """Async warmup then publication, sharing the sync registry and native pool.

    Failure/cancellation requests cleanup. If draining fails or is interrupted,
    the name remains reserved until close_pool(name) completes successfully.
    """
    entry = _reserve(config, name)
    try:
        await entry.pool.warmup()
        return _publish(name, entry)
    except BaseException as original:
        try:
            _closing(entry)
            await entry.pool.close()
        except BaseException as cleanup:
            _cleanup_note(original, cleanup)
        else:
            _remove(name, entry)
        raise


def get_pool(name: str = "default") -> Pool:
    """Return the registered instance; never open a pool implicitly.

    Opening and closing names are unavailable for new lookups.
    """
    _name(name)
    with _guard():
        entry = _entries.get(name)
        if entry is None or entry.state != "ready":
            raise _error(PoolNotInitializedError, f"Pool {name!r} is not initialized")
        return entry.pool


def close_pool_sync(name: str = "default") -> None:
    """Stop new lookups, drain and remove the pool. An absent name is a no-op.

    A timeout/error keeps the name reserved; retry close to finish draining.
    """
    entry = _lookup_close(name)
    if entry is not None:
        entry.pool.close_sync()
        _remove(name, entry)


async def close_pool(name: str = "default") -> None:
    """Async drain and removal. Cancellation retains the reservation for retry."""
    entry = _lookup_close(name)
    if entry is not None:
        await entry.pool.close()
        _remove(name, entry)
