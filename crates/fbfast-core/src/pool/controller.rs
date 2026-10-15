//! One independent control thread per live slot; one armed operation, no job queue.
use super::state::{Inner, Progress};
use crate::error::{Error, ErrorKind};
use fbfast_native::CancelHandle;
use std::{
    cell::RefCell,
    sync::{Arc, Condvar, Mutex, Weak},
    thread::JoinHandle,
    time::Instant,
};

#[derive(Clone)]
struct Operation {
    handle: CancelHandle,
    generation: u64,
    deadline: Option<Instant>,
    progress: Arc<Progress>,
    errors: Arc<Mutex<Vec<Error>>>,
    sent: bool,
}

#[derive(Default)]
struct State {
    operation: Option<Operation>,
    in_flight: Option<u64>,
    stop: bool,
}
#[derive(Default)]
struct Mailbox {
    state: Mutex<State>,
    changed: Condvar,
}

pub(super) struct Controller {
    mailbox: Arc<Mailbox>,
    thread: Option<JoinHandle<()>>,
}

pub(super) struct Lease<'a> {
    owner: &'a Controller,
    operation: Operation,
    finished: RefCell<Option<Result<bool, Error>>>,
}

impl Controller {
    pub fn new(inner: Weak<Inner>, index: usize) -> Result<Self, Error> {
        let mailbox = Arc::new(Mailbox::default());
        let cloned = mailbox.clone();
        let thread = std::thread::Builder::new()
            .name(format!("fbfast-control-{index}"))
            .spawn(move || control_loop(cloned, inner))
            .map_err(|e| Error::new(ErrorKind::PoolBusy, "cancel_setup", e.to_string()))?;
        Ok(Self {
            mailbox,
            thread: Some(thread),
        })
    }

    pub fn arm(
        &self,
        handle: CancelHandle,
        generation: u64,
        deadline: Option<Instant>,
        progress: Arc<Progress>,
    ) -> Lease<'_> {
        let operation = Operation {
            handle,
            generation,
            deadline,
            progress,
            errors: Arc::default(),
            sent: false,
        };
        let weak = Arc::downgrade(&self.mailbox);
        operation
            .progress
            .cancellation
            .set_waker(Some(Arc::new(move || {
                if let Some(mailbox) = weak.upgrade() {
                    let _state = mailbox.state.lock().unwrap_or_else(|e| e.into_inner());
                    mailbox.changed.notify_all();
                }
            })));
        self.mailbox
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .operation = Some(operation.clone());
        self.mailbox.changed.notify_all();
        Lease {
            owner: self,
            operation,
            finished: RefCell::new(None),
        }
    }

    pub fn stop_and_join(mut self) -> Result<(), Error> {
        self.stop();
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            return Err(Error::new(
                ErrorKind::Cleanup,
                "cancel_shutdown",
                "Control worker panicked",
            ));
        }
        Ok(())
    }

    fn stop(&self) {
        self.mailbox
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stop = true;
        self.mailbox.changed.notify_all();
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Lease<'_> {
    pub fn is_finished(&self) -> bool {
        self.finished.borrow().is_some()
    }
    pub fn finish(&self) -> Result<bool, Error> {
        if let Some(result) = self.finished.borrow().as_ref() {
            return result.clone();
        }
        self.disarm();
        // Native generation gate rejects a command already copied by the controller
        // but not yet sent. If it is inside cancelOperation, this waits for it.
        let raised = self
            .operation
            .handle
            .finish(self.operation.generation)
            .map_err(Error::from);
        let mut state = self
            .owner
            .mailbox
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while state.in_flight == Some(self.operation.generation) {
            state = self
                .owner
                .mailbox
                .changed
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
        drop(state);
        let mut errors = self
            .operation
            .errors
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let result = if errors.is_empty() {
            raised
        } else {
            let mut error = Error::new(
                ErrorKind::Cleanup,
                "cancel",
                "Native cancellation control failed",
            );
            error.cleanup = std::mem::take(&mut errors);
            if let Err(other) = raised {
                error.cleanup.push(other);
            }
            Err(error)
        };
        *self.finished.borrow_mut() = Some(result.clone());
        result
    }

    fn disarm(&self) {
        self.operation.progress.cancellation.set_waker(None);
        let mut state = self
            .owner
            .mailbox
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if state
            .operation
            .as_ref()
            .is_some_and(|op| op.generation == self.operation.generation)
        {
            state.operation = None;
        }
        self.owner.mailbox.changed.notify_all();
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        // No network or joining from Drop. The owning worker's recovery path
        // invalidates native control before explicit cleanup after a panic.
        self.disarm();
    }
}

fn control_loop(mailbox: Arc<Mailbox>, inner: Weak<Inner>) {
    if let Some(inner) = inner.upgrade() {
        inner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats
            .control_threads += 1;
    }
    loop {
        let mut state = mailbox.state.lock().unwrap_or_else(|e| e.into_inner());
        let operation = loop {
            if state.stop {
                break None;
            }
            if let Some(op) = state.operation.as_mut() {
                if !op.sent {
                    if op.deadline.is_some_and(|end| Instant::now() >= end)
                        && op.progress.cancellation.expire_on_controller()
                    {
                        if let Some(inner) = inner.upgrade() {
                            inner
                                .state
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .stats
                                .timeouts += 1;
                        }
                    }
                    if op.progress.cancellation.check().is_err() {
                        op.sent = true;
                        let operation = op.clone();
                        state.in_flight = Some(operation.generation);
                        break Some(operation);
                    }
                }
            }
            let until = state
                .operation
                .as_ref()
                .filter(|op| !op.sent)
                .and_then(|op| op.deadline);
            state = if let Some(end) = until {
                mailbox
                    .changed
                    .wait_timeout(state, end.saturating_duration_since(Instant::now()))
                    .unwrap_or_else(|e| e.into_inner())
                    .0
            } else {
                mailbox
                    .changed
                    .wait(state)
                    .unwrap_or_else(|e| e.into_inner())
            };
        };
        drop(state);
        let Some(operation) = operation else { break };
        let result = operation.handle.request(operation.generation);
        if let Some(inner) = inner.upgrade() {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(result, Ok(true)) {
                state.stats.native_cancels += 1;
            }
        }
        if let Err(error) = result {
            operation
                .errors
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(error.into());
        }
        let mut state = mailbox.state.lock().unwrap_or_else(|e| e.into_inner());
        state.in_flight = None;
        mailbox.changed.notify_all();
    }
    if let Some(inner) = inner.upgrade() {
        inner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats
            .control_threads -= 1;
    }
}
