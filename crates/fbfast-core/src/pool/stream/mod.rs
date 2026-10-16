mod wait;
use super::{
    fork_error,
    stream_worker::{Demand, Event},
    DeliveryBudget, Output, Pending,
};
use crate::{
    error::{Error, ErrorKind},
    sinks::rows::Rows,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
pub use wait::StreamWait;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Open,
    Ended,
    Closed,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StreamAction {
    Open,
    Next,
    Close,
}
pub enum StreamItem {
    Opened,
    Arrow(arrow_array::RecordBatch),
    Batch(Rows),
    Json(crate::sinks::json::JsonOutput),
    End,
}
pub(super) enum StreamFormat {
    Rows(bool),
    Json(
        (
            crate::sinks::json::JsonOptions,
            crate::sinks::json::JsonFormat,
        ),
    ),
    Arrow,
}
struct Completion {
    pending: Pending,
    result: Option<Result<(), Error>>,
}
struct Shared {
    receiver: flume::Receiver<Event>,
    demand: Arc<Demand>,
    completion: Mutex<Completion>,
    phase: Mutex<Phase>,
    busy: AtomicBool,
    schema: Mutex<Option<arrow_schema::SchemaRef>>,
}

pub struct Stream {
    shared: Option<Arc<Shared>>,
    pid: u32,
}

impl Stream {
    pub fn check_pid(&self) -> Result<(), Error> {
        if self.pid != std::process::id() {
            Err(fork_error())
        } else {
            Ok(())
        }
    }
    pub fn check_idle(&self) -> Result<(), Error> {
        self.check_pid()?;
        if self
            .shared
            .as_ref()
            .expect("live stream")
            .busy
            .load(Ordering::Acquire)
        {
            return Err(Error::new(
                ErrorKind::PoolBusy,
                "stream",
                "Stream already has an active consumer",
            ));
        }
        Ok(())
    }
    pub fn arrow_schema(&self) -> Result<arrow_schema::SchemaRef, Error> {
        self.check_pid()?;
        self.shared
            .as_ref()
            .expect("live stream")
            .schema
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Query,
                    "stream",
                    "Enter an Arrow stream before exporting its schema",
                )
            })
    }
    pub fn stop(&self, cancel: bool) -> Result<(), Error> {
        self.check_pid()?;
        self.shared.as_ref().expect("live stream").stop(cancel);
        Ok(())
    }

    pub(super) fn new(
        pending: Pending,
        receiver: flume::Receiver<Event>,
        demand: Arc<Demand>,
    ) -> Self {
        Self {
            pid: std::process::id(),
            shared: Some(Arc::new(Shared {
                receiver,
                demand,
                completion: Mutex::new(Completion {
                    pending,
                    result: None,
                }),
                phase: Mutex::new(Phase::New),
                busy: AtomicBool::new(false),
                schema: Mutex::new(None),
            })),
        }
    }
    pub fn request(&self, action: StreamAction) -> Result<StreamWait, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        let shared = self.shared.as_ref().expect("stream exists until Drop");
        if shared
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::new(
                ErrorKind::PoolBusy,
                "stream",
                "Stream already has an active consumer",
            ));
        }
        let phase = *shared.phase.lock().unwrap_or_else(|e| e.into_inner());
        if (action == StreamAction::Open && phase != Phase::New)
            || (action == StreamAction::Next && phase == Phase::New)
        {
            shared.busy.store(false, Ordering::Release);
            return Err(Error::new(
                ErrorKind::Query,
                "stream",
                "Enter the stream exactly once before iteration",
            ));
        }
        if action == StreamAction::Close {
            shared.stop(false);
        } else if action == StreamAction::Next && phase == Phase::Open {
            let mut demand = shared
                .demand
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            demand.requested = demand.requested.saturating_add(1);
            shared.demand.changed.notify_all();
        }
        Ok(StreamWait {
            shared: Some(shared.clone()),
            pid: self.pid,
            action,
            done: false,
        })
    }
}

impl Shared {
    fn check_terminal_error(&self) -> Result<(), Error> {
        if let Some(Err(error)) = self.terminal() {
            *self.phase.lock().unwrap_or_else(|e| e.into_inner()) = Phase::Ended;
            Err(error)
        } else {
            Ok(())
        }
    }
    fn stop(&self, cancel: bool) {
        self.demand
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stop = true;
        self.demand.changed.notify_all();
        if cancel {
            self.completion
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending
                .cancel(false);
        }
    }
    fn terminal(&self) -> Option<Result<(), Error>> {
        let mut completion = self.completion.lock().unwrap_or_else(|e| e.into_inner());
        if completion.result.is_none() {
            completion.result = match completion.pending.try_result() {
                Ok(None) => None,
                Ok(Some(Output::Ready)) => Some(Ok(())),
                Ok(Some(_)) => Some(Err(Error::new(
                    ErrorKind::Query,
                    "stream",
                    "Unexpected stream completion",
                ))),
                Err(error) => Some(Err(error)),
            };
        }
        completion.result.clone()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.take() {
            if self.pid != std::process::id() {
                std::mem::forget(shared);
            } else if *shared.phase.lock().unwrap_or_else(|e| e.into_inner()) != Phase::Closed {
                shared.stop(true);
            }
        }
    }
}
