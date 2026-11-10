"""Immutable query descriptions for running several queries at once."""
from __future__ import annotations

from collections.abc import Iterator, Mapping
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, Generic, Literal, TypeVar, cast, overload

from ._errors import ArgumentError, usage
from ._types import DEFAULT, Params, Timeout
from .analytics import Unsupported, _mode
from .json_options import JsonBinary, JsonDecimals, JsonIntegers, JsonNonFinite, JsonOrient
from .row_queries import ModelT, _check_model

if TYPE_CHECKING:
    import polars as pl
    import pyarrow as pa  # type: ignore[import-untyped]

    from .pool import Pool

T = TypeVar("T")


@dataclass(frozen=True, slots=True, eq=False)
class Query(Generic[T]):
    """One query to run with ``pool.run_all`` / ``pool.run_many`` (and ``_async``).

    Build it with the factory matching the result you want, e.g.
    ``Query.fetch_all(sql, params, model=Order)``; ``results[query]`` is then
    typed as ``list[Order]``. Each Query object is a distinct key, so create two
    objects to run the same SQL twice. Streams are not available here.
    """

    kind: str
    sql: str
    params: Params = None
    timeout: Timeout = DEFAULT
    option: Any = None
    model: Any = field(default=None, repr=False)

    @overload
    @staticmethod
    def fetch_all(sql: str, params: Params = None, *, model: type[ModelT], as_dict: bool = False,
                  timeout: Timeout = DEFAULT) -> Query[list[ModelT]]: ...
    @overload
    @staticmethod
    def fetch_all(sql: str, params: Params = None, *, model: None = None,
                  as_dict: Literal[False] = False,
                  timeout: Timeout = DEFAULT) -> Query[list[tuple[Any, ...]]]: ...
    @overload
    @staticmethod
    def fetch_all(sql: str, params: Params = None, *, model: None = None, as_dict: Literal[True],
                  timeout: Timeout = DEFAULT) -> Query[list[dict[str, Any]]]: ...
    @staticmethod
    def fetch_all(sql: str, params: Params = None, *, model: Any = None, as_dict: bool = False,
                  timeout: Timeout = DEFAULT) -> Query[Any]:
        """All rows, like ``Pool.fetch_all``."""
        _check_model(model)
        return Query("rows", sql, params, timeout, (False, as_dict or model is not None), model)

    @overload
    @staticmethod
    def fetch_one(sql: str, params: Params = None, *, model: type[ModelT], as_dict: bool = False,
                  timeout: Timeout = DEFAULT) -> Query[ModelT | None]: ...
    @overload
    @staticmethod
    def fetch_one(sql: str, params: Params = None, *, model: None = None,
                  as_dict: Literal[False] = False,
                  timeout: Timeout = DEFAULT) -> Query[tuple[Any, ...] | None]: ...
    @overload
    @staticmethod
    def fetch_one(sql: str, params: Params = None, *, model: None = None, as_dict: Literal[True],
                  timeout: Timeout = DEFAULT) -> Query[dict[str, Any] | None]: ...
    @staticmethod
    def fetch_one(sql: str, params: Params = None, *, model: Any = None, as_dict: bool = False,
                  timeout: Timeout = DEFAULT) -> Query[Any]:
        """First row or None, like ``Pool.fetch_one``."""
        _check_model(model)
        return Query("rows", sql, params, timeout, (True, as_dict or model is not None), model)

    @staticmethod
    def fetch_val(sql: str, params: Params = None, *, timeout: Timeout = DEFAULT) -> Query[Any]:
        """First column of the first row, or None."""
        return Query("value", sql, params, timeout)

    @staticmethod
    def fetch_json(sql: str, params: Params = None, *, orient: JsonOrient = "records",
                   json_integers: JsonIntegers = "safe", json_decimals: JsonDecimals = "string",
                   json_binary: JsonBinary = "error", json_non_finite: JsonNonFinite = "error",
                   timeout: Timeout = DEFAULT) -> Query[bytes]:
        """Native JSON array bytes, like ``Pool.fetch_json``."""
        options = [orient, json_integers, json_decimals, json_binary, json_non_finite]
        return Query("json", sql, params, timeout, options)

    @staticmethod
    def fetch_arrow(sql: str, params: Params = None, *,
                    timeout: Timeout = DEFAULT) -> Query[pa.Table]:
        """A ``pyarrow.Table``; requires ``fbfast[arrow]``."""
        return Query("arrow", sql, params, timeout, "arrow")

    @staticmethod
    def fetch_polars(sql: str, params: Params = None, *, unsupported: Unsupported = "error",
                     timeout: Timeout = DEFAULT) -> Query[pl.DataFrame]:
        """A ``polars.DataFrame``; requires ``fbfast[polars]``."""
        return Query("arrow", sql, params, timeout, _mode(unsupported))

    @staticmethod
    def explain(sql: str, params: Params = None, *, timeout: Timeout = DEFAULT) -> Query[str]:
        """The execution plan text, without executing the query."""
        return Query("plan", sql, params, timeout)

    def _spec(self, pool: Pool) -> tuple[str, str, Params, float | None, Any]:
        return (self.kind, self.sql, self.params, pool._timeout(self.timeout), self.option)

    def _adapt(self, value: Any) -> Any:
        """Applies the Pydantic model (validation runs after fetching)."""
        if self.model is None:
            return value
        if self.option[0]:
            return None if value is None else self.model.model_validate(value)
        return [self.model.model_validate(row) for row in value]

    async def _execute_async(self, pool: Pool) -> Any:
        first = bool(self.option[0]) if self.kind == "rows" else False
        timeout = pool._timeout(self.timeout)
        native = pool._core
        if self.kind == "rows":
            rows = await native.rows_async(self.sql, self.params, first=first,
                                           as_dict=bool(self.option[1]), timeout=timeout)
            if self.model is None:
                return rows
            import asyncio
            return await asyncio.to_thread(self._adapt, rows)
        if self.kind == "value":
            return await native.value_async(self.sql, self.params, timeout=timeout)
        if self.kind == "plan":
            return await native.explain_async(self.sql, self.params, timeout=timeout)
        if self.kind == "json":
            return await native.json(self.sql, self.params, options=tuple(self.option),
                                     timeout=timeout, asynchronous=True)
        return await native.arrow(self.sql, self.params, mode=self.option, timeout=timeout,
                                  asynchronous=True)


@dataclass(frozen=True, slots=True)
class Done(Generic[T]):
    """One finished query of a group, in completion order."""

    query: Query[T]
    index: int
    _value: Any = field(default=None, repr=False)
    _error: BaseException | None = field(default=None, repr=False)

    def result(self) -> T:
        """The value, or raises the query's error."""
        if self._error is not None:
            raise self._error
        return cast(T, self._value)

    def exception(self) -> BaseException | None:
        """The query's error, or None if it succeeded."""
        return self._error


class Results(Mapping[Query[Any], Any]):
    """Outcome of ``run_all``: ``results[query]`` is typed by the Query.

    Iteration follows the input order. With ``fail_fast=False`` a failed query
    raises its error when looked up; ``errors`` lists them all.
    """

    __slots__ = ("_queries", "_done")

    def __init__(self, queries: list[Query[Any]], done: dict[int, Done[Any]]) -> None:
        self._queries = queries
        self._done = done

    def __getitem__(self, query: Query[T]) -> T:
        for index, candidate in enumerate(self._queries):
            if candidate is query:
                return cast(T, self._done[index].result())
        raise KeyError(query)

    def __iter__(self) -> Iterator[Query[Any]]:
        return iter(self._queries)

    def __len__(self) -> int:
        return len(self._queries)

    @property
    def errors(self) -> dict[Query[Any], BaseException]:
        """Failed queries and their errors (empty with ``fail_fast=True``)."""
        return {self._queries[i]: d._error for i, d in self._done.items() if d._error is not None}


def checked(queries: Any) -> list[Query[Any]]:
    items = list(queries)
    if not all(isinstance(item, Query) for item in items):
        raise usage(ArgumentError, "run_all/run_many accept only fbfast.Query objects")
    if len({id(item) for item in items}) != len(items):
        raise usage(ArgumentError, "Each Query object may appear once; create two Query "
                    "objects to run the same SQL twice")
    return items
