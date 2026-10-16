//! Session exclusivity spans native execution and language-level delivery.
use super::{
    fork_error,
    state::{Inner, Progress},
};
use crate::error::{Error, ErrorKind};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Weak,
};
use std::time::Instant;

pub(super) struct Scope {
    inner: Weak<Inner>,
    pub progress: Arc<Progress>,
    pub ready: AtomicBool,
    pub closing: AtomicBool,
    pub ended: AtomicBool,
    pub deadline: Mutex<Option<Instant>>,
    pub failure: Mutex<Option<Error>>,
    busy: AtomicBool,
    active: Mutex<Option<Arc<Progress>>>,
}

impl Scope {
    pub fn new(inner: Weak<Inner>) -> Arc<Self> {
        let scope = Arc::new(Self {
            inner,
            progress: Arc::new(Progress::default()),
            ready: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            deadline: Mutex::new(None),
            failure: Mutex::new(None),
            busy: AtomicBool::new(false),
            active: Mutex::new(None),
        });
        scope.forward_cancellation();
        scope
    }
    pub fn forward_cancellation(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        self.progress.cancellation.set_waker(Some(Arc::new(move || {
            if let Some(scope) = weak.upgrade() {
                scope.cancel_active();
            }
        })));
    }
    pub fn cancel_active(&self) {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(progress) = active {
            if self
                .progress
                .cancellation
                .check()
                .is_err_and(|e| e.kind == ErrorKind::QueryTimeout)
            {
                progress.cancellation.timeout();
            } else {
                progress.cancellation.cancel();
            }
        }
    }
    pub fn request_close(&self) {
        // Publish closing only after an active delivery sees cancellation.
        // Otherwise the worker may acknowledge successful close first.
        self.cancel_active();
        self.closing.store(true, Ordering::Release);
    }
    pub fn close_result(&self) -> Result<(), Error> {
        // Permit::drop aborts the parent before clearing active; this order
        // also observes cancellation if delivery finishes between the checks.
        self.check_active()?;
        self.check_errors()
    }
    pub fn abort(&self) {
        self.progress.cancellation.cancel();
    }
    pub fn check(&self) -> Result<(), Error> {
        self.check_errors()?;
        if self.ended.load(Ordering::Acquire) || self.closing.load(Ordering::Acquire) {
            return Err(closed());
        }
        Ok(())
    }
    fn check_errors(&self) -> Result<(), Error> {
        if let Some(error) = self
            .failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(error);
        }
        self.progress.cancellation.check()?;
        if self
            .deadline
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|end| Instant::now() >= end)
        {
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
                "snapshot",
                "Snapshot deadline expired",
            ));
        }
        Ok(())
    }
    pub fn check_active(&self) -> Result<(), Error> {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        active.map_or(Ok(()), |progress| progress.cancellation.check())
    }
    pub fn reserve(self: &Arc<Self>, progress: Arc<Progress>) -> Result<Arc<Permit>, Error> {
        self.check()?;
        if !self.ready.load(Ordering::Acquire) {
            return Err(Error::new(
                ErrorKind::Query,
                "snapshot",
                "Enter the snapshot before querying",
            ));
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::new(
                ErrorKind::PoolBusy,
                "snapshot",
                "Snapshot already has an active operation or open stream",
            ));
        }
        *self.active.lock().unwrap_or_else(|e| e.into_inner()) = Some(progress);
        Ok(Arc::new(Permit {
            scope: Some(self.clone()),
            pid: std::process::id(),
            delivered: AtomicBool::new(false),
        }))
    }
}

pub(super) struct Permit {
    scope: Option<Arc<Scope>>,
    pid: u32,
    delivered: AtomicBool,
}
impl Permit {
    pub fn complete(&self) {
        self.delivered.store(true, Ordering::Release);
    }
    pub fn check(&self) -> Result<(), Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        self.scope.as_ref().expect("live permit").check()
    }
    pub fn deadline(&self) -> Option<Instant> {
        *self
            .scope
            .as_ref()
            .expect("live permit")
            .deadline
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        let Some(scope) = self.scope.take() else {
            return;
        };
        if self.pid != std::process::id() {
            std::mem::forget(scope);
            return;
        }
        if !self.delivered.load(Ordering::Acquire) {
            scope.abort();
        }
        *scope.active.lock().unwrap_or_else(|e| e.into_inner()) = None;
        scope.busy.store(false, Ordering::Release);
    }
}

pub(super) fn closed() -> Error {
    Error::new(
        ErrorKind::PoolClosed,
        "snapshot",
        "Snapshot session is closed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_preserves_cancel_or_timeout_before_delivery_permit_is_released() {
        for timeout in [false, true] {
            let scope = Scope::new(Weak::new());
            scope.ready.store(true, Ordering::Release);
            let progress = Arc::new(Progress::default());
            let permit = scope.reserve(progress.clone()).unwrap();
            // A blocking Python materializer still retains the permit. Parent
            // cancellation therefore has not propagated through Permit::drop.
            let retained = permit.clone();
            drop(permit);
            if timeout {
                progress.cancellation.timeout();
            }
            scope.request_close();
            assert!(scope.closing.load(Ordering::Acquire));
            assert!(scope.progress.cancellation.check().is_ok());
            assert_eq!(
                scope.close_result().unwrap_err().kind,
                if timeout {
                    ErrorKind::QueryTimeout
                } else {
                    ErrorKind::Cancelled
                }
            );
            drop(retained);
            assert!(scope.close_result().is_err());
        }
        let idle = Scope::new(Weak::new());
        idle.request_close();
        assert!(idle.close_result().is_ok());
        let expired = Scope::new(Weak::new());
        *expired.deadline.lock().unwrap() = Some(Instant::now());
        expired.request_close();
        assert_eq!(
            expired.close_result().unwrap_err().kind,
            ErrorKind::QueryTimeout
        );
        let aborted = Scope::new(Weak::new());
        aborted.abort();
        aborted.request_close();
        assert_eq!(
            aborted.close_result().unwrap_err().kind,
            ErrorKind::Cancelled
        );
    }
}
