"""Explicit snapshot context; all operations share one reserved native connection."""
from __future__ import annotations

from types import TracebackType

from ._errors import UsageError, add_cleanup_note, usage
from ._native import FirebirdConfig, _Pool
from ._types import deprecated
from .queries import Queries


class Snapshot(Queries):
    """Consistent read session: use ``with pool.snapshot()`` or ``async with pool.snapshot()``.

    Queries and streams share one READ ONLY SNAPSHOT transaction on one
    connection, so they all see the same committed data. Only one operation may
    run at a time, including Python conversion and an open stream; a second one
    raises :class:`ConcurrentUseError`. Any error ends the snapshot. The scope's
    deadline also counts time spent between queries.
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
            raise usage(UsageError, "Enter an active snapshot context before querying")
        return self._native

    def _start(self) -> _Pool:
        if self._entered or self._closed:
            raise usage(UsageError, "Enter a snapshot context exactly once")
        self._entered = True
        self._native = self._owner.snapshot(timeout=self._scope_timeout)
        return self._native

    def __enter__(self) -> Snapshot:
        native = self._start()
        try:
            native.warmup_sync()
        except BaseException as original:
            try:
                self.close()
            except Exception as cleanup:
                add_cleanup_note(original, cleanup, "snapshot")
            raise
        return self

    async def __aenter__(self) -> Snapshot:
        native = self._start()
        try:
            await native.warmup()
        except BaseException as original:
            try:
                await self.aclose()
            except Exception as cleanup:
                add_cleanup_note(original, cleanup, "snapshot")
            raise
        return self

    def close(self) -> None:
        """End the transaction and return the connection to the pool (idempotent)."""
        self._closed = True
        if self._native is not None:
            self._native.close_sync()

    async def aclose(self) -> None:
        """Async :meth:`close`."""
        self._closed = True
        if self._native is not None:
            await self._native.close()

    def close_sync(self) -> None:
        """Deprecated alias of :meth:`close`."""
        deprecated("Snapshot.close_sync()", "Snapshot.close()")
        self.close()

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
            self.close()
        except Exception as cleanup:
            add_cleanup_note(exc, cleanup, "snapshot")

    async def __aexit__(self, exc_type: type[BaseException] | None, exc: BaseException | None,
                        traceback: TracebackType | None) -> None:
        try:
            await self.aclose()
        except Exception as cleanup:
            add_cleanup_note(exc, cleanup, "snapshot")
