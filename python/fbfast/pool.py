"""Public lifecycle; all scheduling, binding and execution belong to Rust."""

from __future__ import annotations

from types import TracebackType

from ._bootstrap import _CLIENT_PATH
from ._native import FirebirdConfig, _Pool
from .queries import Queries
from .row_queries import _DefaultTimeout, _DEFAULT
from .snapshot import Snapshot


class Pool(Queries):
    """Lazy bounded pool. Native handles stay on their owning Rust worker."""

    __slots__ = ("_core", "_config")

    def __init__(self, config: FirebirdConfig) -> None:
        self._config = config
        self._core = _Pool(config, _CLIENT_PATH)

    @property
    def _executor(self) -> _Pool:
        return self._core

    def snapshot(self, *, timeout: float | None | _DefaultTimeout = _DEFAULT) -> Snapshot:
        """Reserve one READ ONLY SNAPSHOT session; omitted timeout uses query_timeout."""
        return Snapshot(self._core, self._config, self._timeout(timeout))

    def snapshot_async(self, *, timeout: float | None | _DefaultTimeout = _DEFAULT) -> Snapshot:
        """Use async with; timeout includes time between queries and Python conversion."""
        return self.snapshot(timeout=timeout)

    def warmup_sync(self) -> None:
        self._core.warmup_sync()

    async def warmup(self) -> None:
        await self._core.warmup()

    async def close(self) -> None:
        await self._core.close()

    async def __aenter__(self) -> Pool:
        await self.warmup()
        return self

    async def __aexit__(
        self, exc_type: type[BaseException] | None, exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        try:
            await self.close()
        except Exception as cleanup:
            if exc is None:
                raise
            exc.add_note(f"fbfast pool cleanup also failed: {cleanup}")
            if hasattr(exc, "cleanup_errors"):
                exc.cleanup_errors.append(cleanup)

    def close_sync(self) -> None:
        self._core.close_sync()

    def clear_statement_cache_sync(
        self, *, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> None:
        self._core.clear_statement_cache_sync(timeout=self._timeout(timeout))

    async def clear_statement_cache(
        self, *, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> None:
        await self._core.clear_statement_cache(timeout=self._timeout(timeout))

    def stats(self) -> dict[str, int | bool]:
        return self._core.stats()

    def __enter__(self) -> Pool:
        self.warmup_sync()
        return self

    def __exit__(
        self, exc_type: type[BaseException] | None, exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        try:
            self.close_sync()
        except Exception as cleanup:
            if exc is None:
                raise
            exc.add_note(f"fbfast pool cleanup also failed: {cleanup}")
            if hasattr(exc, "cleanup_errors"):
                exc.cleanup_errors.append(cleanup)
