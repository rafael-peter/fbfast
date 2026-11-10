//! After a job: reply, deferred cleanup, cache service, retirement and accounting.
use super::{
    state::{Job, Phase, Work},
    worker::Worker,
    worker_lifecycle::{close_connection, phase},
    Output,
};
use crate::{
    error::{Error, ErrorKind},
    execution::{ExecutionConnection, Trace},
};
use std::time::Instant;

/// What later bookkeeping needs once the result itself has been handed over.
struct Outcome {
    ok: bool,
    size: Option<(u64, u64)>,
    replied: bool,
}

fn merge(result: &mut Result<Output, Error>, cleanup: Error) {
    match result {
        Err(error) => error.cleanup.push(cleanup),
        Ok(_) => *result = Err(cleanup),
    }
}

impl Worker {
    /// Returns true when the worker must stop.
    pub(super) fn finish(
        &mut self,
        job: &Job,
        mut result: Result<Output, Error>,
        panicked: bool,
        mut trace: Option<Trace>,
    ) -> bool {
        let mut outcome = Outcome {
            ok: result.is_ok(),
            size: result.as_ref().ok().and_then(Output::size),
            replied: false,
        };
        if let Some(trace) = &mut trace {
            trace.record.ok = outcome.ok;
            trace.record.reply_seconds = job.submitted.elapsed().as_secs_f64();
            if let Some((rows, bytes)) = outcome.size {
                (trace.record.rows, trace.record.result_bytes) = (rows, bytes);
            }
        }
        if outcome.ok
            && self
                .connection
                .as_ref()
                .is_some_and(ExecutionConnection::has_deferred_cleanup)
        {
            // The result is complete and its cursor closed. Hand it over now and
            // run rollback/reset while the slot stays in Cleaning, so no other
            // job can use this attachment before it is clean.
            self.reply_early(job, &mut result, &mut trace, &mut outcome);
            let deferred = self
                .connection
                .as_mut()
                .expect("connection with deferred cleanup")
                .finish_deferred_cleanup();
            if let Some(owner) = self.inner.upgrade() {
                let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
                state.stats.deferred_cleanups += 1;
                state.stats.transaction_restarts += u64::from(*deferred.as_ref().unwrap_or(&false));
            }
            if let Err(error) = deferred {
                result = Err(error);
            }
        }
        if let Err(cleanup) =
            super::cache_invalidation::service(&self.inner, self.index, &mut self.connection)
        {
            merge(&mut result, cleanup);
        }
        if self.connection.is_none() && !outcome.ok {
            self.failures = self.failures.saturating_add(1);
        }
        let closing = panicked
            || self.inner.upgrade().is_none_or(|inner| {
                inner
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .closing
            });
        let expired = self.lifetime > 0.0 && self.born.elapsed().as_secs_f64() >= self.lifetime;
        let unhealthy = self.connection.as_ref().is_some_and(|c| !c.is_healthy());
        let traffic = super::worker_lifecycle::wire_delta(&mut self.connection, &mut self.wire);
        if closing || expired || unhealthy {
            phase(&self.inner, self.index, Phase::Closing);
            let cleanup = close_connection(&self.inner, &mut self.connection);
            super::cache_invalidation::closed(&self.inner, self.index, cleanup.as_ref().err());
            if let Err(cleanup) = cleanup {
                merge(&mut result, cleanup);
            }
        }
        let native_timeout = result
            .as_ref()
            .is_err_and(|e| matches!(e.kind, ErrorKind::QueryTimeout | ErrorKind::PoolTimeout))
            && job.progress.cancellation.timeout();
        if let Some(inner) = self.inner.upgrade() {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(job.work, Work::KeepWarm) && result.is_err() {
                state.stats.maintenance_failures += 1;
            }
            // The server's own statement timeout can win the controller race.
            if native_timeout {
                state.stats.timeouts += 1;
            }
            if unhealthy {
                state.stats.discarded += 1;
            }
            // A cleanup failure after the reply cannot reach the caller, who
            // already holds a complete result; it is counted and the attachment
            // was discarded above.
            if outcome.replied && result.is_err() {
                state.stats.cleanup_errors += 1;
            }
            if !outcome.replied {
                if let Some((rows, bytes)) = outcome.size.filter(|_| result.is_ok()) {
                    state.stats.rows += rows;
                    state.stats.result_bytes += bytes;
                }
                if let Some(trace) = trace.take() {
                    state.push_trace(trace.record);
                }
            }
            state.stats.wire_roundtrips += traffic.roundtrips;
            state.stats.wire_sent_bytes += traffic.sent_bytes;
            state.stats.wire_received_bytes += traffic.received_bytes;
            if let Some(connection) = &mut self.connection {
                let (hits, misses) = connection.take_cache_counts();
                state.stats.statement_hits += hits;
                state.stats.statement_misses += misses;
            }
            self.last_used = Instant::now();
            if outcome.ok && self.connection.is_some() {
                self.last_verified = self.last_used;
            }
            if !closing {
                super::cache_invalidation::available(
                    &mut state,
                    self.index,
                    self.connection.is_some(),
                );
                state.slots[self.index].idle_since = self.last_used;
                inner.dispatch(&mut state);
            }
            inner.changed.notify_all();
        }
        if let Work::Snapshot { scope, opened, .. } = &job.work {
            use std::sync::atomic::Ordering;
            if let Err(error) = &result {
                *scope.failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
                if !scope.ready.load(Ordering::Acquire) {
                    let _ = opened.try_send(Err(error.clone()));
                }
            }
            scope.ended.store(true, Ordering::Release);
        }
        if !outcome.replied {
            let _ = job.reply.send(result);
        }
        closing
    }

    fn reply_early(
        &mut self,
        job: &Job,
        result: &mut Result<Output, Error>,
        trace: &mut Option<Trace>,
        outcome: &mut Outcome,
    ) {
        // Account before replying so stats()/diagnostics() observed right after
        // the caller receives its result already include this operation.
        let cache = self
            .connection
            .as_mut()
            .map(ExecutionConnection::take_cache_counts);
        if let Some(inner) = self.inner.upgrade() {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((rows, bytes)) = outcome.size {
                state.stats.rows += rows;
                state.stats.result_bytes += bytes;
            }
            if let Some((hits, misses)) = cache {
                state.stats.statement_hits += hits;
                state.stats.statement_misses += misses;
            }
            if let Some(trace) = trace.take() {
                state.push_trace(trace.record);
            }
        }
        if let Work::Snapshot { scope, .. } = &job.work {
            // A closed snapshot is reported as ended before its owner returns.
            scope
                .ended
                .store(true, std::sync::atomic::Ordering::Release);
        }
        let value = std::mem::replace(result, Ok(Output::Ready));
        let _ = job.reply.send(value);
        outcome.replied = true;
    }

    /// Idle tick: probe attachments idle for `health_check_after` off the
    /// critical path. This also keeps tunnels and NAT mappings alive.
    pub(super) fn keepalive(&mut self) {
        let stale_ready = self.config.ready_transaction_timeout.is_some_and(|limit| {
            self.connection
                .as_ref()
                .and_then(ExecutionConnection::ready_since)
                .is_some_and(|since| since.elapsed().as_secs_f64() >= limit)
        });
        if self.connection.is_none()
            || (!stale_ready
                && self.last_verified.elapsed().as_secs_f64() < self.config.health_check_after)
        {
            return;
        }
        let Some(owner) = self.inner.upgrade() else {
            return;
        };
        {
            let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.closing || state.slots[self.index].phase != Phase::Idle {
                return;
            }
            // Not dispatchable while probing; the scheduler uses another slot.
            state.slots[self.index].phase = Phase::Cleaning;
        }
        let connection = self.connection.as_mut().expect("idle connection");
        // An unused pre-started transaction is rolled back; that exchange also
        // proves the attachment alive, so no separate ping is needed then.
        let probe = if stale_ready {
            connection.release_ready()
        } else {
            connection.ping()
        };
        let traffic = super::worker_lifecycle::wire_delta(&mut self.connection, &mut self.wire);
        let cleanup = if probe.is_err() {
            phase(&self.inner, self.index, Phase::Closing);
            let cleanup = close_connection(&self.inner, &mut self.connection);
            super::cache_invalidation::closed(&self.inner, self.index, cleanup.as_ref().err());
            cleanup.err()
        } else {
            self.last_verified = Instant::now();
            None
        };
        let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stats.keepalive_pings += u64::from(!stale_ready);
        state.stats.wire_roundtrips += traffic.roundtrips;
        state.stats.wire_sent_bytes += traffic.sent_bytes;
        state.stats.wire_received_bytes += traffic.received_bytes;
        if probe.is_err() {
            state.stats.discarded += 1;
        }
        if let Some(error) = cleanup {
            state.stats.cleanup_errors += 1;
            state.push_close_error(error);
        }
        if !state.closing {
            super::cache_invalidation::available(&mut state, self.index, self.connection.is_some());
            owner.dispatch(&mut state);
        }
        owner.changed.notify_all();
    }
}
