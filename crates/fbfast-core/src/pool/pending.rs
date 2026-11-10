use super::{
    fork_error,
    state::{Inner, Progress},
    Output,
};
use crate::error::{Error, ErrorKind};
use flume::{Receiver, RecvTimeoutError};
use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

pub struct Pending {
    pub(super) scope: Option<Arc<super::session_scope::Permit>>,
    inner: Option<Weak<Inner>>,
    progress: Option<Arc<Progress>>,
    receiver: Option<Receiver<Result<Output, Error>>>,
    id: u64,
    pid: u32,
    finished: bool,
    acquire_deadline: Instant,
    timeout: Option<f64>,
}

impl Pending {
    /// Language bindings acknowledge successful conversion before releasing a session lease.
    pub fn delivery_complete(&self) {
        if self.pid == std::process::id() {
            if let Some(scope) = &self.scope {
                scope.complete();
            }
        }
    }
    pub(super) fn opening_receiver(&self, receiver: Receiver<Result<Output, Error>>) -> Self {
        Self {
            inner: self.inner.clone(),
            progress: self.progress.clone(),
            receiver: Some(receiver),
            id: self.id,
            pid: self.pid,
            finished: false,
            acquire_deadline: self.acquire_deadline,
            timeout: self.timeout,
            scope: None,
        }
    }
    pub(super) fn try_result(&mut self) -> Result<Option<Output>, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        match self.receiver.as_ref().expect("live receiver").try_recv() {
            Ok(result) => {
                self.finished = true;
                result.map(Some)
            }
            Err(flume::TryRecvError::Empty) => Ok(None),
            Err(flume::TryRecvError::Disconnected) => {
                self.finished = true;
                Err(Error::new(
                    ErrorKind::PoolClosed,
                    "worker",
                    "Native worker stopped without a result",
                ))
            }
        }
    }

    pub(super) fn check_waiting(&self) -> Result<(), Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        match self.expired() {
            Some(error) => {
                self.cancel(true);
                Err(error)
            }
            None => Ok(()),
        }
    }
    /// The acquisition deadline before the job started, or the query deadline
    /// after it did; the caller cancels the job when this returns an error.
    fn expired(&self) -> Option<Error> {
        let started = *self
            .progress
            .as_ref()
            .expect("live progress")
            .started
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match started {
            None if Instant::now() >= self.acquire_deadline => Some(Error::new(
                ErrorKind::PoolTimeout,
                "acquire",
                "Pool acquisition deadline expired",
            )),
            Some(start)
                if self
                    .timeout
                    .is_some_and(|seconds| start.elapsed().as_secs_f64() >= seconds) =>
            {
                Some(Error::new(
                    ErrorKind::QueryTimeout,
                    "query",
                    "Query deadline expired; native cleanup may still be pending",
                ))
            }
            _ => None,
        }
    }
    pub(super) fn detach(mut self) {
        // Maintenance has no caller, but remains owned/counted by its worker.
        self.finished = true;
    }
    /// Cancellation-safe bounded-channel wait; no native call or blocking I/O.
    /// The binding supplies an async timer for acquisition/query checks.
    pub async fn receive(&mut self) -> Result<Output, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        if self.finished {
            return Err(Error::new(
                ErrorKind::PoolClosed,
                "delivery",
                "Result already consumed",
            ));
        }
        let result = self
            .receiver
            .as_ref()
            .expect("live receiver")
            .recv_async()
            .await;
        self.finished = true;
        result.map_err(|_| {
            Error::new(
                ErrorKind::PoolClosed,
                "worker",
                "Native worker stopped without a result",
            )
        })?
    }
    pub fn delivery_budget(&self) -> Result<super::DeliveryBudget, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        let progress = self.progress.as_ref().expect("live progress").clone();
        let started = *progress.started.lock().unwrap_or_else(|e| e.into_inner());
        let mut deadline = match started {
            Some(start) => crate::execution::control::deadline_from(start, self.timeout)?,
            None => None,
        };
        if let Some(end) = self.scope.as_ref().and_then(|scope| scope.deadline()) {
            deadline = Some(deadline.map_or(end, |current| current.min(end)));
        }
        Ok(super::DeliveryBudget {
            scope: self.scope.clone(),
            inner: self.inner.as_ref().expect("live pool reference").clone(),
            progress,
            deadline,
            pid: self.pid,
        })
    }
    pub(super) fn new(
        inner: &Arc<Inner>,
        id: u64,
        progress: Arc<Progress>,
        receiver: Receiver<Result<Output, Error>>,
        acquire_deadline: Instant,
        timeout: Option<f64>,
    ) -> Self {
        Self {
            scope: None,
            inner: Some(Arc::downgrade(inner)),
            progress: Some(progress),
            receiver: Some(receiver),
            id,
            pid: std::process::id(),
            finished: false,
            acquire_deadline,
            timeout,
        }
    }

    pub fn wait_slice(&mut self, duration: Duration) -> Result<Option<Output>, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        if self.finished {
            return Err(Error::new(
                ErrorKind::PoolClosed,
                "delivery",
                "Result already consumed",
            ));
        }
        let result = self
            .receiver
            .as_ref()
            .expect("live receiver")
            .recv_timeout(duration);
        match result {
            Ok(result) => {
                self.finished = true;
                result.map(Some)
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.finished = true;
                Err(Error::new(
                    ErrorKind::PoolClosed,
                    "worker",
                    "Native worker stopped without a result",
                ))
            }
            Err(RecvTimeoutError::Timeout) => {
                let error = self.expired();
                if let Some(error) = error {
                    self.cancel(true);
                    self.finished = true;
                    return Err(error);
                }
                Ok(None)
            }
        }
    }

    pub fn cancel(&self, timeout: bool) {
        if self.pid != std::process::id() || self.finished {
            return;
        }
        let token = &self.progress.as_ref().expect("live progress").cancellation;
        let fresh = if timeout {
            token.timeout()
        } else {
            token.cancel()
        };
        if !fresh {
            return;
        }
        if let Some(inner) = self.inner.as_ref().and_then(Weak::upgrade) {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(index) = state.queue.iter().position(|job| job.id == self.id) {
                state.queue.remove(index);
            }
            state
                .cache_waiters
                .retain(|waiter| waiter.job.id != self.id);
            if timeout {
                state.stats.timeouts += 1;
            } else {
                state.stats.cancellations += 1;
            }
            inner.changed.notify_all();
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.pid != std::process::id() {
            if let Some(value) = self.scope.take() {
                std::mem::forget(value);
            }
            if let Some(value) = self.inner.take() {
                std::mem::forget(value);
            }
            if let Some(value) = self.progress.take() {
                std::mem::forget(value);
            }
            if let Some(value) = self.receiver.take() {
                std::mem::forget(value);
            }
        } else {
            self.cancel(false);
        }
    }
}

/// Final outcome of a long-lived operation (a stream or a snapshot session),
/// cached once observed so every later caller sees the same result.
pub(super) struct Completion {
    pub pending: Pending,
    result: Option<Result<(), Error>>,
}

impl Completion {
    pub fn new(pending: Pending) -> Self {
        Self {
            pending,
            result: None,
        }
    }

    /// Non-blocking: the outcome if the worker already reported it.
    pub fn poll(&mut self, phase: &'static str) -> Option<Result<(), Error>> {
        if self.result.is_none() {
            self.result = match self.pending.try_result() {
                Ok(None) => None,
                Ok(Some(output)) => Some(ready(output, phase)),
                Err(error) => Some(Err(error)),
            };
        }
        self.result.clone()
    }

    /// Waits up to `slice` for the outcome without busy polling.
    pub fn wait(&mut self, slice: Duration, phase: &'static str) -> Option<Result<(), Error>> {
        if self.result.is_none() && !self.pending.finished && self.pending.pid == std::process::id()
        {
            match self
                .pending
                .receiver
                .as_ref()
                .expect("live receiver")
                .recv_timeout(slice)
            {
                Ok(result) => {
                    self.pending.finished = true;
                    self.result = Some(result.and_then(|output| ready(output, phase)));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.pending.finished = true;
                    self.result = Some(Err(Error::new(
                        ErrorKind::PoolClosed,
                        "worker",
                        "Native worker stopped without a result",
                    )));
                }
                Err(RecvTimeoutError::Timeout) => (),
            }
        }
        self.poll(phase)
    }
}

fn ready(output: Output, phase: &'static str) -> Result<(), Error> {
    match output {
        Output::Ready => Ok(()),
        _ => Err(Error::new(ErrorKind::Query, phase, "Unexpected completion")),
    }
}
