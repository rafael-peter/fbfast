//! Opt-in per-operation diagnostics: elapsed time and wire round trips per phase.
//! Reading the Remote provider's wire counters is local (no server round trip).
use super::ExecutionConnection;
use fbfast_native::WireStats;
use serde::Serialize;
use std::time::Instant;

/// One measured phase of an operation, in the order it happened.
#[derive(Debug, Clone, Serialize)]
pub struct PhaseTrace {
    pub phase: &'static str,
    pub seconds: f64,
    pub roundtrips: u64,
}

/// Diagnostics for one pool operation, recorded when `diagnostics=True`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct QueryTrace {
    pub kind: &'static str,
    pub ok: bool,
    pub statement_cached: bool,
    /// Submission until the result was handed to the caller.
    pub reply_seconds: f64,
    /// Round trips before the result was handed to the caller.
    pub roundtrips: u64,
    pub sent_bytes: u64,
    pub received_bytes: u64,
    pub rows: u64,
    pub result_bytes: u64,
    pub phases: Vec<PhaseTrace>,
}

pub(crate) struct Trace {
    last_at: Instant,
    last_wire: Option<WireStats>,
    pub(crate) record: QueryTrace,
}

impl Trace {
    pub(crate) fn new(kind: &'static str, submitted: Instant) -> Self {
        Self {
            last_at: submitted,
            last_wire: None,
            record: QueryTrace {
                kind,
                ..QueryTrace::default()
            },
        }
    }

    /// Starts counting wire traffic from `wire` (zero for a brand-new attachment).
    pub(crate) fn baseline(&mut self, wire: Option<WireStats>) {
        self.last_wire = wire;
    }

    pub(crate) fn mark(&mut self, phase: &'static str, wire: Option<WireStats>) {
        let now = Instant::now();
        let delta = match (wire, self.last_wire) {
            (Some(current), Some(previous)) => current.since(&previous),
            _ => WireStats::default(),
        };
        if wire.is_some() {
            self.last_wire = wire;
        }
        self.record.roundtrips += delta.roundtrips;
        self.record.sent_bytes += delta.sent_bytes;
        self.record.received_bytes += delta.received_bytes;
        self.record.phases.push(PhaseTrace {
            phase,
            seconds: now.saturating_duration_since(self.last_at).as_secs_f64(),
            roundtrips: delta.roundtrips,
        });
        self.last_at = now;
    }
}

impl ExecutionConnection {
    /// Records a phase boundary when this operation is traced; otherwise free.
    pub(crate) fn mark(&mut self, phase: &'static str) {
        if let Some(mut trace) = self.trace.take() {
            let wire = self.native.wire_stats().ok().flatten();
            trace.mark(phase, wire);
            self.trace = Some(trace);
        }
    }

    pub(crate) fn start_trace(&mut self, mut trace: Trace, fresh: bool) {
        let wire = if fresh {
            Some(WireStats::default())
        } else {
            self.native.wire_stats().ok().flatten()
        };
        trace.baseline(wire);
        self.trace = Some(trace);
    }

    pub(crate) fn take_trace(&mut self) -> Option<Trace> {
        self.trace.take()
    }
}
