use super::state::Inner;
use crate::{
    error::{Error, ErrorKind},
    execution::{control::Cancellation, BatchConsumer},
    sinks::rows::Rows,
};
use std::{
    sync::{Arc, Condvar, Mutex, Weak},
    time::{Duration, Instant},
};

pub(super) enum Event {
    Opened,
    Batch(Rows),
}
pub(super) struct DemandState {
    pub requested: u64,
    pub stop: bool,
}
pub(super) struct Demand {
    pub state: Mutex<DemandState>,
    pub changed: Condvar,
}
impl Default for Demand {
    fn default() -> Self {
        Self {
            state: Mutex::new(DemandState {
                requested: 0,
                stop: false,
            }),
            changed: Condvar::new(),
        }
    }
}
pub(super) struct Producer {
    pub sender: flume::Sender<Event>,
    pub demand: Arc<Demand>,
}
pub(super) struct Consumer<'a> {
    pub producer: &'a Producer,
    pub inner: &'a Weak<Inner>,
    pub cancellation: &'a Cancellation,
    pub prefetch: u32,
    pub idle_timeout: Option<f64>,
    pub produced: u64,
    pub opened: bool,
}

impl BatchConsumer for Consumer<'_> {
    fn before_fetch(&mut self) -> Result<bool, Error> {
        if !self.opened {
            self.producer
                .sender
                .try_send(Event::Opened)
                .map_err(|_| consumer_gone())?;
            self.opened = true;
        }
        let idle_since = Instant::now();
        loop {
            self.cancellation.check()?;
            if self.inner.upgrade().is_none_or(|inner| {
                inner
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .closing
            }) {
                return Err(Error::new(
                    ErrorKind::PoolClosed,
                    "stream",
                    "Pool closed while streaming",
                ));
            }
            let state = self
                .producer
                .demand
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if state.stop {
                return Ok(false);
            }
            let credit = if state.requested == 0 {
                0
            } else {
                state.requested.saturating_add(self.prefetch as u64)
            };
            if self.produced < credit {
                return Ok(true);
            }
            if self
                .idle_timeout
                .is_some_and(|seconds| idle_since.elapsed().as_secs_f64() >= seconds)
            {
                return Err(Error::new(
                    ErrorKind::QueryTimeout,
                    "stream_idle",
                    "Stream idle deadline expired without demand",
                ));
            }
            drop(
                self.producer
                    .demand
                    .changed
                    .wait_timeout(state, Duration::from_millis(25))
                    .unwrap_or_else(|e| e.into_inner()),
            );
        }
    }
    fn batch(&mut self, rows: Rows) -> Result<bool, Error> {
        let (count, bytes) = (rows.count, rows.bytes);
        self.producer
            .sender
            .try_send(Event::Batch(rows))
            .map_err(|_| consumer_gone())?;
        self.produced += 1;
        if let Some(inner) = self.inner.upgrade() {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stats.rows += count as u64;
            state.stats.result_bytes += bytes as u64;
        }
        Ok(true)
    }
}
fn consumer_gone() -> Error {
    Error::new(
        ErrorKind::Cancelled,
        "stream",
        "Stream consumer closed or exceeded its bounded channel",
    )
}
