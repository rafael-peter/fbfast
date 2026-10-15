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
    Batch(Rows),
    Json(crate::sinks::json::JsonOutput),
    End,
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
}

pub struct Stream {
    shared: Option<Arc<Shared>>,
    pid: u32,
}
pub struct StreamWait {
    shared: Option<Arc<Shared>>,
    pid: u32,
    action: StreamAction,
    done: bool,
}

impl Stream {
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

impl StreamWait {
    pub fn cancel(&self) {
        if self.pid == std::process::id() {
            self.shared.as_ref().expect("live stream").stop(true);
        }
    }
    pub fn budget(&self) -> Result<DeliveryBudget, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        self.shared
            .as_ref()
            .expect("live stream")
            .completion
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .delivery_budget()
    }
    pub fn poll(&mut self) -> Result<Option<StreamItem>, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        let shared = self.shared.as_ref().expect("live stream");
        if self.done {
            return Ok(Some(StreamItem::End));
        }
        if self.action != StreamAction::Close {
            shared.check_terminal_error()?;
            if matches!(
                *shared.phase.lock().unwrap_or_else(|e| e.into_inner()),
                Phase::Ended | Phase::Closed
            ) {
                self.finish();
                return Ok(Some(StreamItem::End));
            }
            shared
                .completion
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending
                .check_waiting()?;
            self.budget()?.check_phase("stream_delivery")?;
        }
        loop {
            match shared.receiver.try_recv() {
                Ok(event) if self.action != StreamAction::Close => {
                    return self.accept(event).map(Some)
                }
                Ok(_) => continue, // close releases bounded prefetched batches
                Err(_) => break,
            }
        }
        if let Some(result) = shared.terminal() {
            if self.action == StreamAction::Close && result.is_ok() {
                shared
                    .completion
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .pending
                    .delivery_complete();
            }
            *shared.phase.lock().unwrap_or_else(|e| e.into_inner()) =
                if self.action == StreamAction::Close {
                    Phase::Closed
                } else {
                    Phase::Ended
                };
            self.finish();
            result.map(|()| Some(StreamItem::End))
        } else {
            Ok(None)
        }
    }
    pub async fn receive(&mut self) -> Result<Option<StreamItem>, Error> {
        if let Some(item) = self.poll()? {
            return Ok(Some(item));
        }
        let event = self
            .shared
            .as_ref()
            .expect("live stream")
            .receiver
            .recv_async()
            .await;
        match event {
            Ok(event) if self.action != StreamAction::Close => self.accept(event).map(Some),
            _ => self.poll(),
        }
    }
    fn accept(&mut self, event: Event) -> Result<StreamItem, Error> {
        self.shared
            .as_ref()
            .expect("live stream")
            .check_terminal_error()?;
        self.budget()?.check_phase("stream_delivery")?;
        let shared = self.shared.as_ref().expect("live stream");
        let item = match (self.action, event) {
            (StreamAction::Open, Event::Opened) => {
                *shared.phase.lock().unwrap_or_else(|e| e.into_inner()) = Phase::Open;
                StreamItem::Opened
            }
            (StreamAction::Next, Event::Batch(rows)) => StreamItem::Batch(rows),
            (StreamAction::Next, Event::Json(json)) => StreamItem::Json(json),
            _ => {
                return Err(Error::new(
                    ErrorKind::Query,
                    "stream",
                    "Unexpected stream protocol event",
                ))
            }
        };
        self.finish();
        Ok(item)
    }
    fn finish(&mut self) {
        self.done = true;
    }
}

impl Drop for StreamWait {
    fn drop(&mut self) {
        if self.pid != std::process::id() {
            if let Some(shared) = self.shared.take() {
                std::mem::forget(shared);
            }
        } else {
            let shared = self.shared.as_ref().expect("live stream");
            if !self.done {
                shared.stop(true);
            }
            shared.busy.store(false, Ordering::Release);
        }
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
