"""Run several queries at once over one pool, bounded by its connections."""
from __future__ import annotations

import asyncio
import time
from types import TracebackType
from typing import TYPE_CHECKING, Any, Self

from ._errors import ArgumentError, QueryTimeoutError, usage
from .query import Done, Query, Results, checked

if TYPE_CHECKING:
    from .pool import Pool


def _limit(pool: Pool, concurrency: int | None) -> int:
    size = pool.config.pool_size
    if concurrency is None:
        return size
    if isinstance(concurrency, bool) or not isinstance(concurrency, int) or concurrency < 1:
        raise usage(ArgumentError, "concurrency must be a positive integer")
    return min(concurrency, size)


class QueryGroup:
    """Iterate over finished queries in completion order (``with pool.run_many(...)``).

    At most ``concurrency`` queries hold a connection at a time; the rest wait
    outside the pool queue, so other callers keep being served. Leaving the
    ``with`` block cancels whatever is still running.
    """

    __slots__ = ("_pool", "_queries", "_native", "_fail_fast", "_stopped")

    def __init__(self, pool: Pool, queries: list[Query[Any]], concurrency: int | None,
                 timeout: float | None, fail_fast: bool) -> None:
        self._pool = pool
        self._queries = queries
        self._fail_fast = fail_fast
        self._stopped = False
        self._native = pool._core.group([query._spec(pool) for query in queries],
                                        concurrency=_limit(pool, concurrency), timeout=timeout)

    def __enter__(self) -> Self:
        return self

    def __exit__(self, exc_type: type[BaseException] | None, exc: BaseException | None,
                 traceback: TracebackType | None) -> None:
        self.cancel()

    def __iter__(self) -> Self:
        return self

    def __next__(self) -> Done[Any]:
        if self._stopped:
            raise StopIteration
        item = self._native.next()
        if item is None:
            self._stopped = True
            raise StopIteration
        index, ok, value = item
        query = self._queries[index]
        if ok:
            try:
                return Done(query, index, query._adapt(value))
            except Exception as error:  # e.g. pydantic.ValidationError
                value = error
        if self._fail_fast:
            self.cancel()
        return Done(query, index, None, value)

    def cancel(self) -> None:
        """Cancel unfinished queries; finished ones already returned are kept."""
        self._stopped = True
        self._native.cancel()

    @property
    def concurrency(self) -> int:
        """Queries allowed in flight at once (never above ``pool_size``)."""
        return int(self._native.concurrency)


class AsyncQueryGroup:
    """Async counterpart of :class:`QueryGroup` (``async with pool.run_many_async(...)``)."""

    __slots__ = ("_pool", "_queries", "_limit", "_deadline", "_fail_fast", "_next",
                 "_running", "_stopped")

    def __init__(self, pool: Pool, queries: list[Query[Any]], concurrency: int | None,
                 timeout: float | None, fail_fast: bool) -> None:
        self._pool = pool
        self._queries = queries
        self._limit = _limit(pool, concurrency)
        self._deadline = None if timeout is None else time.monotonic() + timeout
        self._fail_fast = fail_fast
        self._next = 0
        self._running: dict[asyncio.Task[Any], int] = {}
        self._stopped = False

    async def __aenter__(self) -> Self:
        self._fill()  # the first window starts on entry
        return self

    async def __aexit__(self, exc_type: type[BaseException] | None, exc: BaseException | None,
                        traceback: TracebackType | None) -> None:
        await self.cancel()

    def __aiter__(self) -> Self:
        return self

    def _fill(self) -> None:
        while (not self._stopped and len(self._running) < self._limit
               and self._next < len(self._queries)):
            query = self._queries[self._next]
            self._running[asyncio.ensure_future(query._execute_async(self._pool))] = self._next
            self._next += 1

    async def __anext__(self) -> Done[Any]:
        if self._stopped:
            raise StopAsyncIteration
        self._fill()
        if not self._running:
            self._stopped = True
            raise StopAsyncIteration
        remaining = None if self._deadline is None else max(0.0, self._deadline - time.monotonic())
        finished, _ = await asyncio.wait(self._running, timeout=remaining,
                                         return_when=asyncio.FIRST_COMPLETED)
        if not finished:
            await self.cancel()
            raise QueryTimeoutError("Query group deadline expired; unfinished queries were "
                                    "cancelled", phase="group")
        task = min(finished, key=self._running.__getitem__)
        index = self._running.pop(task)
        query = self._queries[index]
        error = task.exception()
        if error is None:
            return Done(query, index, task.result())
        if self._fail_fast:
            await self.cancel()
        return Done(query, index, None, error)

    async def cancel(self) -> None:
        """Cancel unfinished queries and wait until their tasks have stopped."""
        self._stopped = True
        tasks = list(self._running)
        self._running.clear()
        for task in tasks:
            task.cancel()
        if tasks:
            await asyncio.gather(*tasks, return_exceptions=True)

    @property
    def concurrency(self) -> int:
        """Queries allowed in flight at once (never above ``pool_size``)."""
        return self._limit


def run_all(pool: Pool, queries: Any, concurrency: int | None, timeout: float | None,
            fail_fast: bool) -> Results:
    items = checked(queries)
    done: dict[int, Done[Any]] = {}
    with QueryGroup(pool, items, concurrency, timeout, fail_fast) as group:
        for finished in group:
            if fail_fast and finished.exception() is not None:
                finished.result()
            done[finished.index] = finished
    return Results(items, done)


async def run_all_async(pool: Pool, queries: Any, concurrency: int | None,
                        timeout: float | None, fail_fast: bool) -> Results:
    items = checked(queries)
    done: dict[int, Done[Any]] = {}
    async with AsyncQueryGroup(pool, items, concurrency, timeout, fail_fast) as group:
        async for finished in group:
            if fail_fast and finished.exception() is not None:
                finished.result()
            done[finished.index] = finished
    return Results(items, done)
