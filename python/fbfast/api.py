"""Typed module-level delegates; names resolve to the existing Pool instance."""
from __future__ import annotations

from typing import TYPE_CHECKING, Any

from .analytics import Unsupported

if TYPE_CHECKING:
    import pyarrow as pa  # type: ignore[import-untyped]
    import polars as pl

from .queries import _DEFAULT, _DefaultTimeout
from .registry import get_pool
from .stream import AsyncStream, Stream, ArrowStream, AsyncArrowStream
from .stream import AsyncJsonStream, JsonStream
from .json_options import JsonBinary, JsonDecimals, JsonFormat, JsonIntegers, JsonNonFinite, JsonOrient


def stream_arrow(sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
                 batch_rows: int | None = None, timeout: float | None | _DefaultTimeout = _DEFAULT) -> ArrowStream:
    """Resolve a named pool; use with for Arrow batches or blocking C Stream export."""
    return get_pool(pool).stream_arrow(sql, params, batch_rows=batch_rows, timeout=timeout)


def stream_arrow_async(sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
                       batch_rows: int | None = None, timeout: float | None | _DefaultTimeout = _DEFAULT) -> AsyncArrowStream:
    """Resolve a named pool; use async with and async for owned ArrowBatch objects."""
    return get_pool(pool).stream_arrow_async(sql, params, batch_rows=batch_rows, timeout=timeout)


def fetch_arrow(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
    timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> pa.Table:
    """Materialize a native Arrow table from a named pool; requires fbfast[arrow]."""
    return get_pool(pool).fetch_arrow(sql, params, timeout=timeout)


async def fetch_arrow_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
    timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> pa.Table:
    """Materialize Arrow asynchronously through the named pool's executor."""
    return await get_pool(pool).fetch_arrow_async(sql, params, timeout=timeout)


def fetch_polars(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
    unsupported: Unsupported = "error", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> pl.DataFrame:
    """Materialize Polars from a named pool, with explicit wide-decimal fallback."""
    return get_pool(pool).fetch_polars(sql, params, unsupported=unsupported, timeout=timeout)


async def fetch_polars_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
    unsupported: Unsupported = "error", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> pl.DataFrame:
    """Materialize Polars asynchronously, including final conversion off the loop."""
    return await get_pool(pool).fetch_polars_async(sql, params, unsupported=unsupported, timeout=timeout)


def fetch_json(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
    orient: JsonOrient = "records", json_integers: JsonIntegers = "safe",
    json_decimals: JsonDecimals = "string", json_binary: JsonBinary = "error",
    json_non_finite: JsonNonFinite = "error", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> bytes:
    """Native JSON array from a named pool; precision options match Pool.fetch_json."""
    return get_pool(pool).fetch_json(sql, params, orient=orient, json_integers=json_integers,
                                    json_decimals=json_decimals, json_binary=json_binary,
                                    json_non_finite=json_non_finite, timeout=timeout)


async def fetch_json_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
    orient: JsonOrient = "records", json_integers: JsonIntegers = "safe",
    json_decimals: JsonDecimals = "string", json_binary: JsonBinary = "error",
    json_non_finite: JsonNonFinite = "error", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> bytes:
    """Async native JSON array from a named pool."""
    return await get_pool(pool).fetch_json_async(sql, params, orient=orient, json_integers=json_integers,
                                               json_decimals=json_decimals, json_binary=json_binary,
                                               json_non_finite=json_non_finite, timeout=timeout)


def stream_json(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
    format: JsonFormat = "ndjson", batch_rows: int | None = None,
    orient: JsonOrient = "records", json_integers: JsonIntegers = "safe",
    json_decimals: JsonDecimals = "string", json_binary: JsonBinary = "error",
    json_non_finite: JsonNonFinite = "error", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> JsonStream:
    """Resolve a named pool; enter the context to stream native JSON bytes."""
    return get_pool(pool).stream_json(sql, params, format=format, batch_rows=batch_rows,
                                      orient=orient, json_integers=json_integers,
                                      json_decimals=json_decimals, json_binary=json_binary,
                                      json_non_finite=json_non_finite, timeout=timeout)


def stream_json_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *, pool: str = "default",
    format: JsonFormat = "ndjson", batch_rows: int | None = None,
    orient: JsonOrient = "records", json_integers: JsonIntegers = "safe",
    json_decimals: JsonDecimals = "string", json_binary: JsonBinary = "error",
    json_non_finite: JsonNonFinite = "error", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> AsyncJsonStream:
    """Resolve a named pool; use async with to stream native JSON bytes."""
    return get_pool(pool).stream_json_async(sql, params, format=format, batch_rows=batch_rows,
                                            orient=orient, json_integers=json_integers,
                                            json_decimals=json_decimals, json_binary=json_binary,
                                            json_non_finite=json_non_finite, timeout=timeout)


def fetch_all(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", as_dict: bool = False,
    timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> list[tuple[Any, ...]] | list[dict[str, Any]]:
    """Fetch rows from a named pool; omitted timeout uses its query_timeout."""
    return get_pool(pool).fetch_all(sql, params, as_dict=as_dict, timeout=timeout)


def fetch_one(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", as_dict: bool = False,
    timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> tuple[Any, ...] | dict[str, Any] | None:
    """Fetch the first row or None from a named pool."""
    return get_pool(pool).fetch_one(sql, params, as_dict=as_dict, timeout=timeout)


def fetch_val(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> Any:
    """Fetch the first column of the first row, or None, from a named pool."""
    return get_pool(pool).fetch_val(sql, params, timeout=timeout)


def explain(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> str:
    """Prepare and explain a query through a named pool."""
    return get_pool(pool).explain(sql, params, timeout=timeout)


def stream(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", batch_rows: int | None = None, as_dict: bool = False,
    timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> Stream:
    """Resolve the pool now; enter the returned context to start streaming."""
    return get_pool(pool).stream(sql, params, batch_rows=batch_rows, as_dict=as_dict,
                                 timeout=timeout)


def stream_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", batch_rows: int | None = None, as_dict: bool = False,
    timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> AsyncStream:
    """Resolve the pool now; use async with on the returned context."""
    return get_pool(pool).stream_async(sql, params, batch_rows=batch_rows, as_dict=as_dict,
                                       timeout=timeout)


async def fetch_all_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", as_dict: bool = False,
    timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> list[tuple[Any, ...]] | list[dict[str, Any]]:
    """Fetch rows asynchronously using the named pool's scheduler and limits."""
    return await get_pool(pool).fetch_all_async(sql, params, as_dict=as_dict, timeout=timeout)


async def fetch_one_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", as_dict: bool = False,
    timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> tuple[Any, ...] | dict[str, Any] | None:
    """Fetch the first row or None asynchronously from a named pool."""
    return await get_pool(pool).fetch_one_async(sql, params, as_dict=as_dict, timeout=timeout)


async def fetch_val_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> Any:
    """Fetch the first scalar asynchronously from a named pool."""
    return await get_pool(pool).fetch_val_async(sql, params, timeout=timeout)


async def explain_async(
    sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
    pool: str = "default", timeout: float | None | _DefaultTimeout = _DEFAULT,
) -> str:
    """Prepare and explain asynchronously through a named pool."""
    return await get_pool(pool).explain_async(sql, params, timeout=timeout)
