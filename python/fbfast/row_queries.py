"""Materialized rows and optional Pydantic validation, shared by pools/snapshots."""
from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING, Any, Literal, TypeVar, cast, overload

from ._native import FirebirdConfig, _Pool

if TYPE_CHECKING:
    from pydantic import BaseModel

ModelT = TypeVar("ModelT", bound="BaseModel")


class _DefaultTimeout:
    pass


_DEFAULT = _DefaultTimeout()


def _check_model(model: type[ModelT] | None) -> None:
    if model is None:
        return
    try:
        from pydantic import BaseModel
    except ImportError as exc:
        raise ImportError("Typed rows require Pydantic v2: install fbfast[pydantic]") from exc
    if not isinstance(model, type) or not issubclass(model, BaseModel):
        raise TypeError("model must be a Pydantic v2 BaseModel subclass")
    if not hasattr(model, "model_validate"):
        raise TypeError("Typed rows require Pydantic v2")


def _validate_all(model: type[ModelT], rows: list[dict[str, Any]]) -> list[ModelT]:
    return [model.model_validate(row) for row in rows]


class RowQueries:
    __slots__ = ()
    _config: FirebirdConfig

    @property
    def _executor(self) -> _Pool:
        raise NotImplementedError

    def _timeout(self, value: float | None | _DefaultTimeout) -> float | None:
        return self._config.query_timeout if isinstance(value, _DefaultTimeout) else value

    @overload
    def fetch_all(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: type[ModelT], as_dict: bool = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[ModelT]: ...

    @overload
    def fetch_all(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: Literal[False] = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]]: ...

    @overload
    def fetch_all(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: Literal[True],
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[dict[str, Any]]: ...

    @overload
    def fetch_all(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: bool,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]] | list[dict[str, Any]]: ...

    def fetch_all(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: type[ModelT] | None = None, as_dict: bool = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]] | list[dict[str, Any]] | list[ModelT]:
        """Fetch rows; model selects validated Pydantic objects.

        Model validation follows fetching and is outside the query timeout.
        Async validation runs in a thread; cancellation cannot stop user validators.
        as_dict is redundant when model is supplied. ValidationError propagates.
        max_result_rows and max_result_bytes are enforced before Python conversion.
        """
        _check_model(model)
        rows = self._executor.rows(
            sql, params, as_dict=as_dict or model is not None,
            timeout=self._timeout(timeout),
        )
        if model is None:
            return cast(list[tuple[Any, ...]] | list[dict[str, Any]], rows)
        return _validate_all(model, cast(list[dict[str, Any]], rows))

    @overload
    def fetch_one(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: type[ModelT], as_dict: bool = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> ModelT | None: ...

    @overload
    def fetch_one(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: Literal[False] = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | None: ...

    @overload
    def fetch_one(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: Literal[True],
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> dict[str, Any] | None: ...

    @overload
    def fetch_one(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: bool,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | dict[str, Any] | None: ...

    def fetch_one(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: type[ModelT] | None = None, as_dict: bool = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | dict[str, Any] | None | ModelT:
        """Fetch the first row or None; model selects validated Pydantic objects.

        Model validation follows fetching and is outside the query timeout.
        Async validation runs in a thread; cancellation cannot stop user validators.
        as_dict is redundant when model is supplied. ValidationError propagates.
        """
        _check_model(model)
        rows = self._executor.rows(
            sql, params, first=True, as_dict=as_dict or model is not None,
            timeout=self._timeout(timeout),
        )
        if model is None:
            return cast(tuple[Any, ...] | dict[str, Any] | None, rows)
        if rows is None:
            return None
        return model.model_validate(rows)

    @overload
    async def fetch_all_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: type[ModelT], as_dict: bool = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[ModelT]: ...

    @overload
    async def fetch_all_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: Literal[False] = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]]: ...

    @overload
    async def fetch_all_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: Literal[True],
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[dict[str, Any]]: ...

    @overload
    async def fetch_all_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: bool,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]] | list[dict[str, Any]]: ...

    async def fetch_all_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: type[ModelT] | None = None, as_dict: bool = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]] | list[dict[str, Any]] | list[ModelT]:
        """Fetch rows; model selects validated Pydantic objects.

        Model validation follows fetching and is outside the query timeout.
        Async validation runs in a thread; cancellation cannot stop user validators.
        as_dict is redundant when model is supplied. ValidationError propagates.
        """
        _check_model(model)
        rows = await self._executor.rows_async(
            sql, params, as_dict=as_dict or model is not None,
            timeout=self._timeout(timeout),
        )
        if model is None:
            return cast(list[tuple[Any, ...]] | list[dict[str, Any]], rows)
        return await asyncio.to_thread(_validate_all, model, cast(list[dict[str, Any]], rows))

    @overload
    async def fetch_one_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: type[ModelT], as_dict: bool = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> ModelT | None: ...

    @overload
    async def fetch_one_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: Literal[False] = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | None: ...

    @overload
    async def fetch_one_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: Literal[True],
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> dict[str, Any] | None: ...

    @overload
    async def fetch_one_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: None = None, as_dict: bool,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | dict[str, Any] | None: ...

    async def fetch_one_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        model: type[ModelT] | None = None, as_dict: bool = False,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | dict[str, Any] | None | ModelT:
        """Fetch the first row or None; model selects validated Pydantic objects.

        Model validation follows fetching and is outside the query timeout.
        Async validation runs in a thread; cancellation cannot stop user validators.
        as_dict is redundant when model is supplied. ValidationError propagates.
        """
        _check_model(model)
        rows = await self._executor.rows_async(
            sql, params, first=True, as_dict=as_dict or model is not None,
            timeout=self._timeout(timeout),
        )
        if model is None:
            return cast(tuple[Any, ...] | dict[str, Any] | None, rows)
        if rows is None:
            return None
        return await asyncio.to_thread(model.model_validate, rows)

