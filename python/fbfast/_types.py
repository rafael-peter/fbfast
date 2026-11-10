"""Public typing helpers: timeout sentinel, parameter type and result dictionaries."""
from __future__ import annotations

import warnings
from typing import Any, Final, TypeAlias, TypedDict


class DefaultTimeout:
    """Type of :data:`DEFAULT`: "use the configured timeout" (``query_timeout`` or ``snapshot_timeout``)."""

    __slots__ = ()

    def __repr__(self) -> str:
        return "fbfast.DEFAULT"

    def __reduce__(self) -> str:
        return "DEFAULT"


DEFAULT: Final = DefaultTimeout()
"""Omitted ``timeout=``: use the configured default. ``None`` disables the deadline."""

Timeout: TypeAlias = float | None | DefaultTimeout
"""Seconds (float), ``None`` for no deadline, or :data:`DEFAULT`."""

Params: TypeAlias = list[Any] | tuple[Any, ...] | None
"""Positional values for the ``?`` placeholders, in order."""


class PoolStats(TypedDict):
    """Snapshot of pool counters returned by ``Pool.stats()``.

    Slot phases (``dormant`` ... ``closed``) count connections right now; the
    other integers are cumulative since the pool was created.
    """

    cache_invalidations: int
    maintenance_failures: int
    control_threads: int
    native_cancels: int
    dormant: int
    opening: int
    idle: int
    leased: int
    cleaning: int
    closing: int
    closed: int
    waiting: int
    acquires: int
    timeouts: int
    cancellations: int
    discarded: int
    connections_created: int
    statement_hits: int
    statement_misses: int
    rows: int
    result_bytes: int
    closing_pool: bool
    cleanup_errors: int
    wire_roundtrips: int
    wire_sent_bytes: int
    wire_received_bytes: int
    deferred_cleanups: int
    keepalive_pings: int
    reconnects: int
    transaction_restarts: int


class PhaseTrace(TypedDict):
    """One measured phase of a traced query."""

    phase: str
    seconds: float
    roundtrips: int


class QueryTrace(TypedDict):
    """Diagnostics for one pool query (``FirebirdConfig(diagnostics=True)``)."""

    kind: str
    ok: bool
    statement_cached: bool
    reply_seconds: float
    roundtrips: int
    sent_bytes: int
    received_bytes: int
    rows: int
    result_bytes: int
    phases: list[PhaseTrace]


class SessionInfo(TypedDict):
    """Capabilities observed on a real attachment, returned by ``Pool.session_info()``."""

    server_version: str
    role: str
    charset: str
    time_zone: str
    wire_encrypted: bool
    wire_compressed: bool
    network_protocol: str
    protocol_version: int
    inline_blobs: bool
    max_inline_blob_size: int
    blob_cache_size: int


def deprecated(old: str, new: str) -> None:
    """Warns once per call site that ``old`` was renamed in 0.3.0."""
    warnings.warn(f"{old} is deprecated since fbfast 0.3.0 and will be removed in 0.4.0; "
                  f"use {new}", DeprecationWarning, stacklevel=3)
