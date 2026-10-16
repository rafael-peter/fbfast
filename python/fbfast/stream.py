"""Exclusive stream context managers; scheduling and bounded prefetch live in Rust."""
from __future__ import annotations

from types import TracebackType
from typing import Any, Generic, Self, TypeVar, cast
from .json_options import JsonSettings, JsonFormat

from ._native import _Pool, _Stream, ArrowBatch, ArrowSchema

Batch = list[tuple[Any, ...]] | list[dict[str, Any]]
_T = TypeVar("_T")


def _cleanup_note(original: BaseException | None, cleanup: Exception) -> None:
    if original is None:
        raise cleanup
    original.add_note(f"fbfast stream cleanup also failed: {cleanup}")
    if hasattr(original, "cleanup_errors"):
        original.cleanup_errors.append(cleanup)


class _Base(Generic[_T]):
    __slots__ = ("_owner", "_sql", "_params", "_batch_rows", "_as_dict", "_timeout", "_native", "_entered", "_closed", "_json", "_arrow")

    def __init__(self, owner: _Pool, sql: str, params: list[Any] | tuple[Any, ...] | None,
                 batch_rows: int | None, as_dict: bool, timeout: float | None,
                 json: tuple[JsonSettings, JsonFormat] | None = None, *, arrow: bool = False) -> None:
        self._arrow = arrow
        self._json = json
        self._owner = owner  # Keeps the pool alive for this context's entire lifetime.
        self._sql = sql
        self._params = tuple(params) if isinstance(params, (list, tuple)) else params
        self._batch_rows = batch_rows
        self._as_dict = as_dict
        self._timeout = timeout
        self._native: _Stream | None = None
        self._entered = False
        self._closed = False

    def _start(self) -> _Stream:
        if self._entered or self._closed:
            raise RuntimeError("Enter a stream context exactly once")
        self._entered = True
        if self._json is None:
            self._native = self._owner.stream(self._sql, self._params, batch_rows=self._batch_rows,
                                             as_dict=self._as_dict, timeout=self._timeout, arrow=self._arrow)
        else:
            self._native = self._owner.stream_json(self._sql, self._params, options=self._json,
                                                  batch_rows=self._batch_rows,
                                                  timeout=self._timeout)
        return self._native

    def _active(self) -> _Stream:
        if not self._entered or self._native is None:
            raise RuntimeError("Enter the stream context before iteration")
        return self._native


class _SyncStream(_Base[_T]):
    """Use with pool.stream(...); each iteration returns one owned Python batch."""
    __slots__ = ()

    def __enter__(self) -> Self:
        try:
            self._start().open_sync()
        except BaseException as original:
            try:
                self.close()
            except Exception as cleanup:
                _cleanup_note(original, cleanup)
            raise
        return self

    def __iter__(self) -> Self:
        return self

    def __next__(self) -> _T:
        if self._closed:
            raise StopIteration
        batch = self._active().next_sync()
        if batch is None:
            self._closed = True
            raise StopIteration
        return cast(_T, batch)

    def close(self) -> None:
        self._closed = True
        if self._native is not None:
            self._native.close_sync()
            self._native = None

    def __exit__(self, exc_type: type[BaseException] | None, exc: BaseException | None,
                 traceback: TracebackType | None) -> None:
        try:
            self.close()
        except Exception as cleanup:
            _cleanup_note(exc, cleanup)


class _AsyncStream(_Base[_T]):
    """Use async with pool.stream_async(...), then async for batch in stream."""
    __slots__ = ()

    async def __aenter__(self) -> Self:
        try:
            await self._start().open_async()
        except BaseException as original:
            try:
                await self.aclose()
            except Exception as cleanup:
                _cleanup_note(original, cleanup)
            raise
        return self

    def __aiter__(self) -> Self:
        return self

    async def __anext__(self) -> _T:
        if self._closed:
            raise StopAsyncIteration
        batch = await self._active().next_async()
        if batch is None:
            self._closed = True
            raise StopAsyncIteration
        return cast(_T, batch)

    async def aclose(self) -> None:
        self._closed = True
        if self._native is not None:
            await self._native.close_async()
            self._native = None

    async def __aexit__(self, exc_type: type[BaseException] | None, exc: BaseException | None,
                        traceback: TracebackType | None) -> None:
        try:
            await self.aclose()
        except Exception as cleanup:
            _cleanup_note(exc, cleanup)


class Stream(_SyncStream[Batch]):
    """Synchronous context manager yielding owned Python row batches."""
    __slots__ = ()


class AsyncStream(_AsyncStream[Batch]):
    """Asynchronous context manager yielding owned Python row batches."""
    __slots__ = ()


class JsonStream(_SyncStream[bytes]):
    """Synchronous context manager yielding native UTF-8 JSON byte chunks."""
    __slots__ = ()


class AsyncJsonStream(_AsyncStream[bytes]):
    """Asynchronous context manager yielding native JSON bytes, without Python rows."""
    __slots__ = ()


class ArrowStream(_SyncStream[ArrowBatch]):
    """Owned Arrow batches; a single C Stream export takes over iteration.

    C Stream consumption blocks the calling thread. Use AsyncArrowStream for
    event-loop iteration. The surrounding context owns bounded cleanup.
    """
    __slots__ = ()

    @property
    def schema(self) -> ArrowSchema:
        """Describe-derived schema, available after enter even for empty results."""
        return self._active().schema

    def __arrow_c_stream__(self, requested_schema: object | None = None) -> object:
        if self._closed:
            raise RuntimeError("Cannot export a closed Arrow stream")
        return self._active().__arrow_c_stream__(requested_schema)


class AsyncArrowStream(_AsyncStream[ArrowBatch]):
    """Async context yielding owned ArrowBatch objects without requiring PyArrow."""
    __slots__ = ()

    @property
    def schema(self) -> ArrowSchema:
        """Stable schema, available after async enter before any batch is requested."""
        return self._active().schema
