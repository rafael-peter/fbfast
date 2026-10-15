"""Public lifecycle; all scheduling, binding and execution belong to Rust."""

from __future__ import annotations

from types import TracebackType
from typing import Any, cast

from ._bootstrap import _CLIENT_PATH
from ._native import FirebirdConfig, _Pool


class _DefaultTimeout:
    pass


_DEFAULT = _DefaultTimeout()


class Pool:
    """Lazy bounded pool. Native handles stay on their owning Rust worker."""

    __slots__ = ("_core", "_config")

    def __init__(self, config: FirebirdConfig) -> None:
        self._config = config
        self._core = _Pool(config, _CLIENT_PATH)

    def _timeout(self, value: float | None | _DefaultTimeout) -> float | None:
        return self._config.query_timeout if isinstance(value, _DefaultTimeout) else value

    def fetch_all(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        as_dict: bool = False, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]] | list[dict[str, Any]]:
        return cast(list[tuple[Any, ...]] | list[dict[str, Any]], self._core.rows(sql, params, as_dict=as_dict, timeout=self._timeout(timeout)))

    def fetch_one(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        as_dict: bool = False, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | dict[str, Any] | None:
        return cast(tuple[Any, ...] | dict[str, Any] | None, self._core.rows(sql, params, first=True, as_dict=as_dict, timeout=self._timeout(timeout)))

    def fetch_val(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> Any:
        return self._core.value(sql, params, timeout=self._timeout(timeout))

    def explain(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> str:
        return self._core.explain(sql, params, timeout=self._timeout(timeout))

    def warmup_sync(self) -> None:
        self._core.warmup_sync()

    async def fetch_all_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        as_dict: bool = False, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]] | list[dict[str, Any]]:
        return cast(list[tuple[Any, ...]] | list[dict[str, Any]],
                    await self._core.rows_async(sql, params, as_dict=as_dict, timeout=self._timeout(timeout)))

    async def fetch_one_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        as_dict: bool = False, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | dict[str, Any] | None:
        return cast(tuple[Any, ...] | dict[str, Any] | None,
                    await self._core.rows_async(sql, params, first=True, as_dict=as_dict, timeout=self._timeout(timeout)))

    async def fetch_val_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> Any:
        return await self._core.value_async(sql, params, timeout=self._timeout(timeout))

    async def explain_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> str:
        return await self._core.explain_async(sql, params, timeout=self._timeout(timeout))

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
