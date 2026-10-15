use super::{
    fork_error,
    state::{Inner, Progress},
};
use crate::error::{Error, ErrorKind};
use std::{
    sync::{Arc, Weak},
    time::Instant,
};

/// Extends the same operation deadline through language-level materialization.
/// It owns no attachment or slot; native cleanup has already completed.
#[derive(Clone)]
pub struct DeliveryBudget {
    pub(super) inner: Weak<Inner>,
    pub(super) progress: Arc<Progress>,
    pub(super) deadline: Option<Instant>,
    pub(super) pid: u32,
}

impl DeliveryBudget {
    pub fn cancel(&self) {
        if self.pid == std::process::id() && self.progress.cancellation.cancel() {
            if let Some(inner) = self.inner.upgrade() {
                inner
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .stats
                    .cancellations += 1;
            }
        }
    }
    pub fn check(&self) -> Result<(), Error> {
        self.check_phase("python_conversion")
    }
    pub fn check_phase(&self, phase: &'static str) -> Result<(), Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        if self.deadline.is_some_and(|end| Instant::now() >= end) {
            if self.progress.cancellation.timeout() {
                if let Some(inner) = self.inner.upgrade() {
                    inner
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .stats
                        .timeouts += 1;
                }
            }
            return Err(Error::new(
                ErrorKind::QueryTimeout,
                phase,
                "Operation deadline expired before result delivery",
            ));
        }
        self.progress.cancellation.check()
    }
}
