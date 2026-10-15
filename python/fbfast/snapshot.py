"""Explicit snapshot context; all operations share one reserved native connection."""
from __future__ import annotations

from types import TracebackType
from ._native import FirebirdConfig, _Pool
from .queries import Queries


def _note(original: BaseException | None, cleanup: Exception) -> None:
    if original is None:
        raise cleanup
    original.add_note(f"fbfast snapshot cleanup also failed: {cleanup}")
    if hasattr(original, "cleanup_errors"):
        original.cleanup_errors.append(cleanup)


class Snapshot(Queries):
    """Use with pool.snapshot() or async with pool.snapshot_async().

    Queries and streams share a READ ONLY SNAPSHOT transaction. Only one operation
    may run at a time, including Python conversion and an open stream. The scope's
    timeout spans time between queries. Pool queries remain independent.
    """
    __slots__ = ("_owner", "_config", "_scope_timeout", "_native", "_entered", "_closed")

    def __init__(self, owner: _Pool, config: FirebirdConfig, timeout: float | None) -> None:
        self._owner = owner
        self._config = config
        self._scope_timeout = timeout
        self._native: _Pool | None = None
        self._entered = False
        self._closed = False

    @property
    def _executor(self) -> _Pool:
        if self._native is None or self._closed:
            raise RuntimeError("Enter an active snapshot context before querying")
        return self._native

    def _start(self) -> _Pool:
        if self._entered or self._closed:
            raise RuntimeError("Enter a snapshot context exactly once")
        self._entered = True
        self._native = self._owner.snapshot(timeout=self._scope_timeout)
        return self._native

    def __enter__(self) -> Snapshot:
        native = self._start()
        try:
            native.warmup_sync()
        except BaseException as original:
            try:
                self.close_sync()
            except Exception as cleanup:
                _note(original, cleanup)
            raise
        return self

    async def __aenter__(self) -> Snapshot:
        native = self._start()
        try:
            await native.warmup()
        except BaseException as original:
            try:
                await self.close()
            except Exception as cleanup:
                _note(original, cleanup)
            raise
        return self

    def close_sync(self) -> None:
        self._closed = True
        if self._native is not None:
            self._native.close_sync()

    async def close(self) -> None:
        self._closed = True
        if self._native is not None:
            await self._native.close()

    def __del__(self) -> None:
        # A retained child stream may still own the native executor. Abandoning
        # this scope must request cleanup anyway; this method never waits for I/O.
        try:
            native = getattr(self, "_native", None)
            if native is not None:
                native.request_close()
        except Exception:
            # Includes the native PID guard; inherited handles are never touched.
            pass

    def __exit__(self, exc_type: type[BaseException] | None, exc: BaseException | None,
                 traceback: TracebackType | None) -> None:
        try:
            self.close_sync()
        except Exception as cleanup:
            _note(exc, cleanup)

    async def __aexit__(self, exc_type: type[BaseException] | None, exc: BaseException | None,
                        traceback: TracebackType | None) -> None:
        try:
            await self.close()
        except Exception as cleanup:
            _note(exc, cleanup)
