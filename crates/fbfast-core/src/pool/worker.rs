//! One native worker thread per slot; it alone owns its attachment.
use super::{
    controller::Controller,
    state::{Command, Inner, Job, Phase, Work},
    worker_lifecycle::{acquire_timeout, close_connection, phase, retire_idle},
    Output,
};
use crate::{
    config::Config,
    error::{Error, ErrorKind},
    execution::{control::deadline_from, ExecutionConnection, Trace},
};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{Receiver, RecvTimeoutError},
        Weak,
    },
    time::{Duration, Instant},
};

/// Worker-local state of one slot. Nothing here is shared with other threads.
pub(super) struct Worker {
    pub inner: Weak<Inner>,
    pub index: usize,
    pub config: Config,
    client: PathBuf,
    pub connection: Option<ExecutionConnection>,
    pub born: Instant,
    /// Last time the slot served work (drives `idle_timeout` retirement).
    pub last_used: Instant,
    /// Last successful exchange with the server (drives `health_check_after`).
    pub last_verified: Instant,
    pub lifetime: f64,
    pub failures: u32,
    controller: Option<Controller>,
    pub wire: fbfast_native::WireStats,
}

pub(super) fn run(
    inner: Weak<Inner>,
    index: usize,
    receiver: Receiver<Command>,
    config: Config,
    client: PathBuf,
) {
    let now = Instant::now();
    let mut worker = Worker {
        inner,
        index,
        lifetime: config.max_lifetime,
        config,
        client,
        connection: None,
        born: now,
        last_used: now,
        last_verified: now,
        failures: 0,
        controller: None,
        wire: fbfast_native::WireStats::default(),
    };
    loop {
        let command = match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(command) => command,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                let w = &worker;
                if retire_idle(
                    &w.inner,
                    w.index,
                    &w.config,
                    w.last_used,
                    w.born,
                    w.lifetime,
                ) {
                    break;
                }
                worker.keepalive();
                continue;
            }
        };
        let job = match command {
            Command::Job(job) => job,
            Command::ClearCache => {
                if super::worker_lifecycle::idle_cache(
                    &worker.inner,
                    worker.index,
                    &mut worker.connection,
                ) {
                    break;
                }
                continue;
            }
            Command::Close => break,
        };
        let (result, panicked, trace) = worker.execute(&job);
        if worker.finish(&job, result, panicked, trace) {
            break;
        }
    }
    worker.shutdown();
}

impl Worker {
    /// Runs one job. A failure before any SQL reached the server (ping or
    /// transaction start on a dead attachment) reconnects once within the
    /// acquisition deadline; a failure after that is returned, never repeated.
    fn execute(&mut self, job: &Job) -> (Result<Output, Error>, bool, Option<Trace>) {
        let mut trace = job
            .work
            .kind()
            .filter(|_| self.config.diagnostics)
            .map(|kind| Trace::new(kind, job.submitted));
        if let Some(trace) = &mut trace {
            trace.mark("queue", None);
        }
        let mut attempt = 0;
        loop {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.attempt(job, &mut trace, attempt)
            }));
            let Ok(result) = result else {
                if let Some(connection) = &mut self.connection {
                    connection.poison();
                }
                let error = Error::new(
                    ErrorKind::Query,
                    "worker",
                    "Native worker panicked; attachment is being discarded",
                );
                return (Err(error), true, self.take_trace(trace));
            };
            if attempt == 0 && self.retry_before_sql(job, &result) {
                attempt += 1;
                trace = self.take_trace(trace);
                if let Some(trace) = &mut trace {
                    trace.mark("failed_attempt", None);
                }
                let cleanup = close_connection(&self.inner, &mut self.connection);
                super::cache_invalidation::closed(&self.inner, self.index, cleanup.as_ref().err());
                if let Some(owner) = self.inner.upgrade() {
                    let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.stats.discarded += 1;
                    state.stats.reconnects += 1;
                }
                continue;
            }
            return (result, false, self.take_trace(trace));
        }
    }

    fn take_trace(&mut self, trace: Option<Trace>) -> Option<Trace> {
        self.connection
            .as_mut()
            .and_then(ExecutionConnection::take_trace)
            .or(trace)
    }

    fn retry_before_sql(&self, job: &Job, result: &Result<Output, Error>) -> bool {
        let Err(error) = result else {
            return false;
        };
        matches!(error.phase, "begin" | "ping")
            && matches!(error.kind, ErrorKind::Query)
            && job.work.kind().is_some()
            && self.connection.as_ref().is_some_and(|c| !c.is_healthy())
            && job.progress.cancellation.check().is_ok()
            && Instant::now() < job.acquire_deadline
    }

    fn attempt(
        &mut self,
        job: &Job,
        trace: &mut Option<Trace>,
        attempt: u32,
    ) -> Result<Output, Error> {
        let (inner, index) = (self.inner.clone(), self.index);
        let mut acquire_deadline = job.acquire_deadline;
        job.progress.cancellation.check()?;
        match inner.upgrade() {
            Some(owner)
                if owner
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .closing =>
            {
                return Err(Error::new(
                    ErrorKind::PoolClosed,
                    "acquire",
                    "Pool closed during acquisition",
                ));
            }
            Some(_) => (),
            None => {
                return Err(Error::new(
                    ErrorKind::PoolClosed,
                    "acquire",
                    "Pool was dropped",
                ))
            }
        }
        if self.connection.is_none() {
            phase(&inner, index, Phase::Opening);
            if self.failures > 0 {
                // Per-slot bounded backoff with deterministic independent jitter; no query replay.
                let millis =
                    (50u64 << self.failures.min(5)) + ((index as u64 * 37 + job.id * 17) % 53);
                std::thread::sleep(Duration::from_millis(millis));
                if matches!(job.work, Work::KeepWarm) {
                    // No caller waits for background maintenance. Backoff is
                    // separate from each retry's connection readiness budget.
                    acquire_deadline =
                        Instant::now() + Duration::from_secs_f64(self.config.acquire_timeout);
                }
            }
            if Instant::now() >= acquire_deadline {
                return Err(acquire_timeout());
            }
            let mut opened = ExecutionConnection::connect_before(
                self.config.clone(),
                &self.client,
                acquire_deadline,
            )?;
            opened.set_defer_cleanup(true);
            self.connection = Some(opened);
            self.opened(job.id);
            if let Some(started) = trace.take() {
                let fresh = self.connection.as_mut().expect("opened connection");
                fresh.start_trace(started, true);
                fresh.mark("connect");
            }
        }
        let connection = self.connection.as_mut().expect("opened connection");
        if let Some(started) = trace.take() {
            connection.start_trace(started, false);
        }
        // The idle tick normally keeps this fresh; check here only if it could not.
        if self.last_verified.elapsed().as_secs_f64() >= self.config.health_check_after {
            connection.ping()?;
            connection.mark("ping");
            self.last_verified = Instant::now();
        }
        job.progress.cancellation.check()?;
        if Instant::now() >= acquire_deadline {
            return Err(acquire_timeout());
        }
        if self.controller.is_none() {
            self.controller = Some(Controller::new(inner.clone(), index)?);
        }
        let started = Instant::now();
        let end = deadline_from(started, job.work.timeout())?;
        *job.progress
            .started
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(started);
        phase(&inner, index, Phase::Leased);
        if attempt == 0 && !matches!(job.work, Work::KeepWarm) {
            if let Some(owner) = inner.upgrade() {
                owner
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .stats
                    .acquires += 1;
            }
        }
        let controller = self.controller.as_ref().expect("controller initialized");
        if matches!(job.work, Work::Snapshot { .. }) {
            return super::session_worker::run(&inner, index, connection, job, controller, end);
        }
        let control = connection.begin_control(job.id)?;
        let lease = controller.arm(control, job.id, end, job.progress.clone());
        let mut result = super::worker_query::execute(&inner, connection, job, end, &|| {
            phase(&inner, index, Phase::Cleaning);
            lease.finish()
        });
        super::worker_lifecycle::settle_lease(&lease, connection, &mut result);
        result
    }

    fn opened(&mut self, job_id: u64) {
        let now = Instant::now();
        (self.born, self.last_used, self.last_verified) = (now, now, now);
        self.wire = fbfast_native::WireStats::default();
        self.lifetime = self.config.max_lifetime
            * (0.9 + ((self.index as u64 * 37 + job_id % 100) % 100) as f64 / 1000.0);
        self.failures = 0;
        if let Some(owner) = self.inner.upgrade() {
            let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stats.connections_created += 1;
            state.session = self.connection.as_ref().map(|c| c.session_info().clone());
        }
    }

    fn shutdown(self) {
        let mut connection = self.connection;
        let cleanup = close_connection(&self.inner, &mut connection);
        super::cache_invalidation::closed(&self.inner, self.index, cleanup.as_ref().err());
        let control_cleanup = self.controller.map(Controller::stop_and_join).transpose();
        if let Some(inner) = self.inner.upgrade() {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            for error in [cleanup.err(), control_cleanup.err()].into_iter().flatten() {
                state.push_close_error(error);
                state.stats.cleanup_errors += 1;
            }
            state.slots[self.index].sender = None;
            state.slots[self.index].phase = Phase::Closed;
            super::cache_invalidation::empty(&mut state, self.index);
            inner.dispatch(&mut state);
            inner.changed.notify_all();
        }
    }
}
