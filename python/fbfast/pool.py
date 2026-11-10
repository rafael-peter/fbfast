"""Public lifecycle; all scheduling, binding and execution belong to Rust."""

from __future__ import annotations

from collections.abc import Iterable
from types import TracebackType
from typing import Any, cast

from ._bootstrap import _CLIENT_PATH
from ._errors import add_cleanup_note
from ._native import FirebirdConfig, _Pool
from ._types import DEFAULT, DefaultTimeout, PoolStats, QueryTrace, SessionInfo, Timeout, deprecated
from .group import AsyncQueryGroup, QueryGroup, run_all, run_all_async
from .queries import Queries
from .query import Query, Results, checked
from .snapshot import Snapshot


class Pool(Queries):
    """Bounded connection pool; every query borrows one connection and returns it.

    Construction is lazy: no client loading, network or threads until the first
    query or :meth:`warmup`. Use ``with Pool(config) as pool`` (or ``async with``)
    so connections are always closed. Methods without a suffix block; methods
    ending in ``_async`` (and :meth:`aclose`) are coroutines.
    """

    __slots__ = ("_core", "_config")

    def __init__(self, config: FirebirdConfig) -> None:
        self._config = config
        self._core = _Pool(config, _CLIENT_PATH)

    @property
    def _executor(self) -> _Pool:
        return self._core

    @property
    def config(self) -> FirebirdConfig:
        """The immutable configuration this pool was created with."""
        return self._config

    def snapshot(self, *, timeout: Timeout = DEFAULT) -> Snapshot:
        """Reserve one connection with a READ ONLY SNAPSHOT transaction.

        Use ``with pool.snapshot() as snap:`` or ``async with pool.snapshot() as snap:``.
        Every query inside sees the same committed data and costs about one
        round trip, which also makes it the fastest way to run several small
        queries over a slow link. Only one operation may run at a time.

        Args:
            timeout: Deadline for the whole scope, including time spent in Python
                between queries. Omitted uses ``snapshot_timeout`` (300 s);
                ``None`` disables it. Each query still uses ``query_timeout``.
        """
        scope = self._config.snapshot_timeout if isinstance(timeout, DefaultTimeout) else timeout
        return Snapshot(self._core, self._config, scope)

    def snapshot_async(self, *, timeout: Timeout = DEFAULT) -> Snapshot:
        """Deprecated: ``pool.snapshot()`` already supports ``async with``."""
        deprecated("Pool.snapshot_async()", "Pool.snapshot() with async with")
        return self.snapshot(timeout=timeout)

    def run_all(self, queries: Iterable[Query[Any]], *, concurrency: int | None = None,
                timeout: float | None = None, fail_fast: bool = True) -> Results:
        """Run independent queries at once and wait for all of them.

        ``results[query]`` returns each value, typed by its :class:`Query`.
        Faster than one query after another when the link has latency: with
        50 ms per round trip, N small queries take about
        ``ceil(N / concurrency)`` times one query.

        Args:
            queries: Distinct :class:`Query` objects (``Query.fetch_all(...)``...).
            concurrency: Queries holding a connection at once; default and
                maximum ``pool_size``. The rest wait outside the pool queue.
            timeout: Deadline for the whole group in seconds (None: no group
                deadline). Each query keeps its own ``timeout``.
            fail_fast: Cancel the rest and raise on the first error; with False
                every query runs and ``results[query]`` raises its own error.
        """
        return run_all(self, queries, concurrency, timeout, fail_fast)

    def run_many(self, queries: Iterable[Query[Any]], *, concurrency: int | None = None,
                 timeout: float | None = None, fail_fast: bool = True) -> QueryGroup:
        """Like :meth:`run_all`, but yield each :class:`Done` as soon as it finishes.

        Use ``with pool.run_many(queries) as group: for done in group: ...``.
        Leaving the block cancels unfinished queries.
        """
        return QueryGroup(self, checked(queries), concurrency, timeout, fail_fast)

    async def run_all_async(self, queries: Iterable[Query[Any]], *,
                            concurrency: int | None = None, timeout: float | None = None,
                            fail_fast: bool = True) -> Results:
        """Async :meth:`run_all`; avoids the queue limit of a plain ``asyncio.gather``."""
        return await run_all_async(self, queries, concurrency, timeout, fail_fast)

    def run_many_async(self, queries: Iterable[Query[Any]], *, concurrency: int | None = None,
                       timeout: float | None = None, fail_fast: bool = True) -> AsyncQueryGroup:
        """Async :meth:`run_many`: ``async with ... as group: async for done in group``."""
        return AsyncQueryGroup(self, checked(queries), concurrency, timeout, fail_fast)

    def warmup(self) -> None:
        """Open ``min_idle`` connections (at least one) now, raising on failure."""
        self._core.warmup_sync()

    async def warmup_async(self) -> None:
        """Async :meth:`warmup`; connection work runs off the event loop."""
        await self._core.warmup()

    def close(self) -> None:
        """Stop accepting work, wait up to ``close_timeout`` and close every connection.

        Raises:
            PoolTimeoutError: Native work did not finish in time; it stays counted.
            CleanupError: A connection could not be closed cleanly.
        """
        self._core.close_sync()

    async def aclose(self) -> None:
        """Async :meth:`close`, waiting without blocking the event loop."""
        await self._core.close()

    def clear_statement_cache(self, *, timeout: Timeout = DEFAULT) -> None:
        """Free every cached prepared statement, e.g. after DDL changed a table."""
        self._core.clear_statement_cache_sync(timeout=self._timeout(timeout))

    async def clear_statement_cache_async(self, *, timeout: Timeout = DEFAULT) -> None:
        """Async :meth:`clear_statement_cache`."""
        await self._core.clear_statement_cache(timeout=self._timeout(timeout))

    def stats(self) -> PoolStats:
        """Current slot phases and cumulative counters, without any I/O."""
        return cast(PoolStats, self._core.stats())

    def diagnostics(self) -> list[QueryTrace]:
        """Drain per-query records (time and round trips per phase).

        Records are kept only with ``FirebirdConfig(diagnostics=True)``; at most
        the latest 1024 are retained. Reading the counters adds no round trip.
        """
        return cast(list[QueryTrace], self._core.diagnostics())

    def session_info(self) -> SessionInfo | None:
        """Capabilities observed by the latest attachment, or None before any connected.

        Includes negotiated wire encryption and compression, protocol version
        and whether small BLOBs travel inline with their rows.
        """
        return cast("SessionInfo | None", self._core.session_info())

    def warmup_sync(self) -> None:
        """Deprecated alias of :meth:`warmup`."""
        deprecated("Pool.warmup_sync()", "Pool.warmup()")
        self.warmup()

    def close_sync(self) -> None:
        """Deprecated alias of :meth:`close`."""
        deprecated("Pool.close_sync()", "Pool.close()")
        self.close()

    def clear_statement_cache_sync(self, *, timeout: Timeout = DEFAULT) -> None:
        """Deprecated alias of :meth:`clear_statement_cache`."""
        deprecated("Pool.clear_statement_cache_sync()", "Pool.clear_statement_cache()")
        self.clear_statement_cache(timeout=timeout)

    def __enter__(self) -> Pool:
        self.warmup()
        return self

    def __exit__(
        self, exc_type: type[BaseException] | None, exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        try:
            self.close()
        except Exception as cleanup:
            add_cleanup_note(exc, cleanup, "pool")

    async def __aenter__(self) -> Pool:
        await self.warmup_async()
        return self

    async def __aexit__(
        self, exc_type: type[BaseException] | None, exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        try:
            await self.aclose()
        except Exception as cleanup:
            add_cleanup_note(exc, cleanup, "pool")
