"""Shared query surface for pools and explicit snapshot sessions."""
from __future__ import annotations

from typing import Any, TYPE_CHECKING, cast
from .analytics import Unsupported, _mode
if TYPE_CHECKING:
    import pyarrow as pa  # type: ignore[import-untyped]
    import polars as pl
from ._native import FirebirdConfig, _Pool
from .stream import AsyncStream, Stream, ArrowStream, AsyncArrowStream
from .stream import AsyncJsonStream, JsonStream
from .json_options import JsonBinary, JsonDecimals, JsonFormat, JsonIntegers, JsonNonFinite, JsonOrient

class _DefaultTimeout:
    pass

_DEFAULT = _DefaultTimeout()

class Queries:
    __slots__ = ()
    _config: FirebirdConfig

    @property
    def _executor(self) -> _Pool:
        raise NotImplementedError

    def _timeout(self, value: float | None | _DefaultTimeout) -> float | None:
        return self._config.query_timeout if isinstance(value, _DefaultTimeout) else value

    def stream_arrow(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
                     batch_rows: int | None = None, timeout: float | None | _DefaultTimeout = _DEFAULT) -> ArrowStream:
        """Use with; yields owned ArrowBatch objects or exports one blocking C Stream.

        No Python Arrow extra is required. Schema is available after entering.
        batch_rows defaults to fetch_size; batch_bytes bounds native batches.
        max_result_rows applies to materialized fetches, not this stream.
        """
        return ArrowStream(self._executor, sql, params, batch_rows, False, self._timeout(timeout), arrow=True)

    def stream_arrow_async(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
                           batch_rows: int | None = None, timeout: float | None | _DefaultTimeout = _DEFAULT) -> AsyncArrowStream:
        """Use async with, then async for ArrowBatch; C Array exports need no PyArrow."""
        return AsyncArrowStream(self._executor, sql, params, batch_rows, False, self._timeout(timeout), arrow=True)

    def fetch_arrow(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
                    timeout: float | None | _DefaultTimeout = _DEFAULT) -> pa.Table:
        """Build a complete pyarrow.Table natively; requires the arrow extra.

        Preserves SQL types and metadata, including full INT128 as Decimal256.
        Uses max_result_rows. No Python tuples or dictionaries are built per row.
        """
        return self._executor.arrow(sql, params, timeout=self._timeout(timeout))

    async def fetch_arrow_async(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
                                timeout: float | None | _DefaultTimeout = _DEFAULT) -> pa.Table:
        """Async fetch_arrow; native work and final adaptation run off the event loop."""
        return await self._executor.arrow(sql, params, timeout=self._timeout(timeout), asynchronous=True)

    def fetch_polars(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
                     unsupported: Unsupported = "error", timeout: float | None | _DefaultTimeout = _DEFAULT) -> pl.DataFrame:
        """Build a Polars DataFrame through Arrow with rechunk=False; no pyarrow required.

        unsupported='error' rejects unsupported Decimal256 columns before fetching.
        'string' explicitly preserves those columns as exact decimal text.
        The received Polars schema is checked before returning the result.
        """
        return cast("pl.DataFrame", self._executor.arrow(sql, params, mode=_mode(unsupported), timeout=self._timeout(timeout)))

    async def fetch_polars_async(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
                                 unsupported: Unsupported = "error", timeout: float | None | _DefaultTimeout = _DEFAULT) -> pl.DataFrame:
        """Async fetch_polars; conversion uses the bounded off-loop delivery executor."""
        return cast("pl.DataFrame", await self._executor.arrow(sql, params, mode=_mode(unsupported),
                    timeout=self._timeout(timeout), asynchronous=True))

    def fetch_json(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        orient: JsonOrient = "records", json_integers: JsonIntegers = "safe",
        json_decimals: JsonDecimals = "string", json_binary: JsonBinary = "error",
        json_non_finite: JsonNonFinite = "error",
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> bytes:
        """Serialize a complete compact UTF-8 JSON array directly in Rust.

        Args:
            orient: records uses unique column names as keys; values emits arrays.
            json_integers: safe emits BIGINT/INT128 as strings by column type; number emits exact JSON numbers, which JavaScript may round.
            json_decimals: string preserves exact decimals by default; number writes exact numeric text without converting through float.
            json_binary: error rejects binary cells; base64 explicitly encodes them.
            json_non_finite: error rejects NaN/Infinity; null explicitly maps them to null.
            timeout: Omitted uses query_timeout; None disables this query's deadline.
        """
        return cast(bytes, self._executor.json(sql, params, options=(orient, json_integers,
                    json_decimals, json_binary, json_non_finite), timeout=self._timeout(timeout)))

    async def fetch_json_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        orient: JsonOrient = "records", json_integers: JsonIntegers = "safe",
        json_decimals: JsonDecimals = "string", json_binary: JsonBinary = "error",
        json_non_finite: JsonNonFinite = "error",
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> bytes:
        """Async fetch_json with the same native writer, precision and row limit."""
        return cast(bytes, await self._executor.json(sql, params, options=(orient, json_integers,
                    json_decimals, json_binary, json_non_finite), timeout=self._timeout(timeout),
                    asynchronous=True))

    def stream_json(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        format: JsonFormat = "ndjson", batch_rows: int | None = None,
        orient: JsonOrient = "records", json_integers: JsonIntegers = "safe",
        json_decimals: JsonDecimals = "string", json_binary: JsonBinary = "error",
        json_non_finite: JsonNonFinite = "error",
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> JsonStream:
        """Use with: ndjson keeps records intact; array chunks may split JSON/UTF-8.

        batch_rows limits native fetch rows; batch_bytes bounds encoded chunks.
        One oversized NDJSON record stays in a single chunk. Materialized-result
        row limits do not apply. Other options match fetch_json.
        """
        return JsonStream(self._executor, sql, params, batch_rows, False, self._timeout(timeout),
                          ((orient, json_integers, json_decimals, json_binary, json_non_finite), format))

    def stream_json_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        format: JsonFormat = "ndjson", batch_rows: int | None = None,
        orient: JsonOrient = "records", json_integers: JsonIntegers = "safe",
        json_decimals: JsonDecimals = "string", json_binary: JsonBinary = "error",
        json_non_finite: JsonNonFinite = "error",
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> AsyncJsonStream:
        """Use async with, then async for bytes; same options as stream_json."""
        return AsyncJsonStream(self._executor, sql, params, batch_rows, False, self._timeout(timeout),
                               ((orient, json_integers, json_decimals, json_binary, json_non_finite), format))


    def fetch_all(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        as_dict: bool = False, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]] | list[dict[str, Any]]:
        return cast(list[tuple[Any, ...]] | list[dict[str, Any]], self._executor.rows(sql, params, as_dict=as_dict, timeout=self._timeout(timeout)))


    def fetch_one(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        as_dict: bool = False, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | dict[str, Any] | None:
        return cast(tuple[Any, ...] | dict[str, Any] | None, self._executor.rows(sql, params, first=True, as_dict=as_dict, timeout=self._timeout(timeout)))


    def fetch_val(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> Any:
        return self._executor.value(sql, params, timeout=self._timeout(timeout))


    def explain(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> str:
        return self._executor.explain(sql, params, timeout=self._timeout(timeout))


    def stream(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
               batch_rows: int | None = None, as_dict: bool = False,
               timeout: float | None | _DefaultTimeout = _DEFAULT) -> Stream:
        return Stream(self._executor, sql, params, batch_rows, as_dict, self._timeout(timeout))


    def stream_async(self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
                     batch_rows: int | None = None, as_dict: bool = False,
                     timeout: float | None | _DefaultTimeout = _DEFAULT) -> AsyncStream:
        return AsyncStream(self._executor, sql, params, batch_rows, as_dict, self._timeout(timeout))


    async def fetch_all_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        as_dict: bool = False, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> list[tuple[Any, ...]] | list[dict[str, Any]]:
        return cast(list[tuple[Any, ...]] | list[dict[str, Any]],
                    await self._executor.rows_async(sql, params, as_dict=as_dict, timeout=self._timeout(timeout)))


    async def fetch_one_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        as_dict: bool = False, timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> tuple[Any, ...] | dict[str, Any] | None:
        return cast(tuple[Any, ...] | dict[str, Any] | None,
                    await self._executor.rows_async(sql, params, first=True, as_dict=as_dict, timeout=self._timeout(timeout)))


    async def fetch_val_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> Any:
        return await self._executor.value_async(sql, params, timeout=self._timeout(timeout))


    async def explain_async(
        self, sql: str, params: list[Any] | tuple[Any, ...] | None = None, *,
        timeout: float | None | _DefaultTimeout = _DEFAULT,
    ) -> str:
        return await self._executor.explain_async(sql, params, timeout=self._timeout(timeout))


