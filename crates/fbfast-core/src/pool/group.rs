//! Completion signal shared by a group of queries: one thread can wait for
//! whichever query finishes first without polling every reply channel.
use super::{Output, Pending, Pool, Query};
use crate::error::Error;
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

#[derive(Default)]
pub struct Signal {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl Signal {
    /// Changes every time a member of the group replies or is dropped.
    pub fn generation(&self) -> u64 {
        *self.generation.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Blocks until the generation differs from `seen` or `timeout` passes.
    pub fn wait(&self, seen: u64, timeout: Duration) -> u64 {
        let guard = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |generation| *generation == seen)
            .unwrap_or_else(|e| e.into_inner());
        *guard
    }

    pub(super) fn notify(&self) {
        let mut generation = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        *generation = generation.wrapping_add(1);
        self.changed.notify_all();
    }
}

type Outcome = Result<Output, Error>;

/// Reply channel of one job. A group member also wakes its group's waiter,
/// including when the job is dropped without a reply.
pub(super) struct Reply {
    sender: flume::Sender<Outcome>,
    signal: Option<Arc<Signal>>,
}

impl Reply {
    pub fn new(sender: flume::Sender<Outcome>, signal: Option<Arc<Signal>>) -> Self {
        Self { sender, signal }
    }

    /// Returns false if the caller already stopped waiting.
    pub fn send(&self, result: Outcome) -> bool {
        let sent = self.sender.send(result).is_ok();
        self.wake();
        sent
    }

    /// Never blocks; false if the caller is gone or already holds a reply.
    pub fn try_send(&self, result: Outcome) -> bool {
        let sent = self.sender.try_send(result).is_ok();
        self.wake();
        sent
    }

    fn wake(&self) {
        if let Some(signal) = &self.signal {
            signal.notify();
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.wake();
    }
}

impl Pool {
    /// Like `submit`, but the reply also wakes every waiter of `signal`.
    pub fn submit_grouped(&self, query: Query, signal: &Arc<Signal>) -> Result<Pending, Error> {
        let inner = self.inner()?;
        query.validate()?;
        inner.submit_signaled(super::state::Work::Query(query), Some(signal.clone()))
    }
}
