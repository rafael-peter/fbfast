//! Coalesced cache epochs, acknowledged by each owner after its active cursor.
use super::{
    state::{Command, Inner, Job, Phase, State, Work},
    Output, Pending,
};
use crate::{
    error::{Error, ErrorKind},
    execution::ExecutionConnection,
};
use std::{
    sync::{Arc, Weak},
    time::Instant,
};

pub(super) struct Waiter {
    pub job: Job,
    errors: Vec<Error>,
}

impl Inner {
    pub(super) fn request_cache_clear(
        self: &Arc<Self>,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closing {
            return Err(Error::new(
                ErrorKind::PoolClosed,
                "cache_clear",
                "Pool is closing or closed",
            ));
        }
        if state.queue.len() + state.cache_waiters.len() >= self.config.max_waiters.max(1) as usize
        {
            return Err(Error::new(
                ErrorKind::PoolBusy,
                "cache_clear",
                "Maintenance waiting capacity is full",
            ));
        }
        if state.next_id == u64::MAX {
            return Err(Error::new(
                ErrorKind::PoolClosed,
                "cache_clear",
                "Operation generations exhausted",
            ));
        }
        let (job, pending) = self.job(&mut state, Work::ClearCache { timeout });
        *job.progress
            .started
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        let epoch = job.id;
        state.cache_epoch = epoch;
        state.stats.cache_invalidations += 1;
        state.cache_waiters.push(Waiter {
            job,
            errors: Vec::new(),
        });
        for slot in &mut state.slots {
            if matches!(slot.phase, Phase::Closed | Phase::Dormant) {
                slot.cache_epoch = epoch;
            } else if slot.phase == Phase::Idle {
                // Reserve before sending: foreground dispatch cannot lease a slot
                // whose idle-cache cleanup is now owned by its worker.
                slot.phase = Phase::Cleaning;
                if let Some(sender) = &slot.sender {
                    let _ = sender.try_send(Command::ClearCache);
                }
            }
        }
        complete(&mut state);
        self.changed.notify_all();
        Ok(pending)
    }
}

/// Only called on the owner, after query cleanup or from an idle control command.
/// Broken connections are acknowledged only after their explicit close path.
pub(super) fn service(
    inner: &Weak<Inner>,
    index: usize,
    connection: &mut Option<ExecutionConnection>,
) -> Result<(), Error> {
    let Some(owner) = inner.upgrade() else {
        return Ok(());
    };
    let required = {
        let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.slots[index].cache_epoch >= state.cache_epoch {
            return Ok(());
        }
        state.slots[index].phase = Phase::Cleaning;
        state.cache_epoch
    };
    drop(owner);
    let result = match connection.as_mut() {
        Some(connection) if connection.is_healthy() => connection.clear_statement_cache(),
        Some(_) => return Ok(()), // close/discard first
        None => Ok(()),
    };
    acknowledge(inner, index, required, result.as_ref().err());
    result
}

pub(super) fn closed(inner: &Weak<Inner>, index: usize, error: Option<&Error>) {
    // All epochs issued before this locked acknowledgement see an empty cache.
    acknowledge(inner, index, u64::MAX, error);
}

fn acknowledge(inner: &Weak<Inner>, index: usize, required: u64, error: Option<&Error>) {
    if let Some(owner) = inner.upgrade() {
        let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
        // No query can run between service() and this acknowledgement. Requests
        // issued during cleanup are covered as well, so one command coalesces them.
        let epoch = state.cache_epoch;
        debug_assert!(required == u64::MAX || required <= epoch);
        state.slots[index].cache_epoch = epoch;
        if let Some(error) = error {
            for waiter in &mut state.cache_waiters {
                waiter.errors.push(error.clone());
            }
        }
        complete(&mut state);
        owner.changed.notify_all();
    }
}

fn complete(state: &mut State) {
    let epoch = state
        .slots
        .iter()
        .map(|slot| slot.cache_epoch)
        .min()
        .unwrap_or(state.cache_epoch);
    let mut index = 0;
    while index < state.cache_waiters.len() {
        if state.cache_waiters[index]
            .job
            .progress
            .cancellation
            .check()
            .is_err()
        {
            state.cache_waiters.remove(index);
        } else if state.cache_waiters[index].job.id <= epoch {
            let waiter = state.cache_waiters.remove(index);
            let result = if waiter.errors.is_empty() {
                Ok(Output::Ready)
            } else {
                let mut error = Error::new(
                    ErrorKind::Cleanup,
                    "cache_clear",
                    "Statement cache invalidation failed on an attachment",
                );
                error.cleanup = waiter.errors;
                Err(error)
            };
            let _ = waiter.job.reply.send(result);
        } else {
            index += 1;
        }
    }
}
