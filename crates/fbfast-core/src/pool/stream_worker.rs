use super::state::Inner;
use crate::{
    error::{Error, ErrorKind},
    execution::{control::Cancellation, BatchConsumer},
    sinks::json::JsonOutput,
    sinks::rows::Rows,
};
use std::{
    sync::{Arc, Condvar, Mutex, Weak},
    time::{Duration, Instant},
};

pub(super) enum Event {
    Opened(Option<arrow_schema::SchemaRef>),
    Arrow(arrow_array::RecordBatch),
    Batch(Rows),
    Json(JsonOutput),
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
    /// Wakes a blocked synchronous consumer after every event.
    pub signal: Arc<super::Signal>,
}

impl Producer {
    fn send(&self, event: Event) -> Result<(), Error> {
        self.sender.try_send(event).map_err(|_| consumer_gone())?;
        self.signal.notify();
        Ok(())
    }
}
pub(super) struct Consumer<'a> {
    pub producer: &'a Producer,
    pub inner: &'a Weak<Inner>,
    pub cancellation: &'a Cancellation,
    pub prefetch: u32,
    pub idle_timeout: Option<f64>,
    pub produced: u64,
    pub opened: bool,
    pub deadline: Option<Instant>,
}

impl Consumer<'_> {
    /// Blocks (bounded, cancellation-aware) until the consumer granted credit.
    fn wait_for_demand(&mut self) -> Result<bool, Error> {
        if !self.opened {
            self.producer.send(Event::Opened(None))?;
            self.opened = true;
        }
        let idle_since = Instant::now();
        loop {
            self.cancellation.check()?;
            if self.deadline.is_some_and(|end| Instant::now() >= end) {
                return Err(Error::new(
                    ErrorKind::QueryTimeout,
                    "query",
                    "Query deadline expired",
                ));
            }
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
    fn send(&mut self, event: Event, count: usize, bytes: usize) -> Result<bool, Error> {
        self.producer.send(event)?;
        self.produced += 1;
        if let Some(inner) = self.inner.upgrade() {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stats.rows += count as u64;
            state.stats.result_bytes += bytes as u64;
        }
        Ok(true)
    }
}
/// A batch the stream worker forwards, with its (rows, bytes) for statistics.
pub(super) trait Streamed {
    fn into_event(self) -> (Event, usize, usize);
}
impl Streamed for Rows {
    fn into_event(self) -> (Event, usize, usize) {
        let (count, bytes) = (self.count, self.bytes);
        (Event::Batch(self), count, bytes)
    }
}
impl Streamed for JsonOutput {
    fn into_event(self) -> (Event, usize, usize) {
        let (count, bytes) = (self.rows, self.data.len());
        (Event::Json(self), count, bytes)
    }
}
impl Streamed for arrow_array::RecordBatch {
    fn into_event(self) -> (Event, usize, usize) {
        let (count, bytes) = (self.num_rows(), self.get_array_memory_size());
        (Event::Arrow(self), count, bytes)
    }
}
impl<B: Streamed> BatchConsumer<B> for Consumer<'_> {
    fn before_fetch(&mut self) -> Result<bool, Error> {
        self.wait_for_demand()
    }
    fn batch(&mut self, batch: B) -> Result<bool, Error> {
        let (event, count, bytes) = batch.into_event();
        self.send(event, count, bytes)
    }
}
impl crate::execution::ArrowConsumer for Consumer<'_> {
    fn schema(&mut self, schema: arrow_schema::SchemaRef) -> Result<(), Error> {
        self.producer.send(Event::Opened(Some(schema)))?;
        self.opened = true;
        Ok(())
    }
}
fn consumer_gone() -> Error {
    Error::new(
        ErrorKind::Cancelled,
        "stream",
        "Stream consumer closed or exceeded its bounded channel",
    )
}
