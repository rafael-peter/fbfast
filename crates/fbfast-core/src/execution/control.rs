use crate::error::{Error, ErrorKind};
use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Cancellation {
    reason: AtomicU8,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Cancellation {
    pub fn cancel(&self) -> bool {
        self.request(1, true)
    }
    pub fn timeout(&self) -> bool {
        self.request(2, true)
    }
    pub(crate) fn expire_on_controller(&self) -> bool {
        self.request(2, false)
    }
    fn request(&self, reason: u8, notify: bool) -> bool {
        let fresh = self
            .reason
            .compare_exchange(0, reason, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if fresh && notify {
            let wake = self.wake.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(wake) = wake {
                wake();
            }
        }
        fresh
    }
    pub(crate) fn set_waker(&self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = wake;
    }
    pub fn check(&self) -> Result<(), Error> {
        match self.reason.load(Ordering::Acquire) {
            1 => Err(Error::new(
                ErrorKind::Cancelled,
                "query",
                "Operation cancelled",
            )),
            2 => Err(Error::new(
                ErrorKind::QueryTimeout,
                "query",
                "Query deadline expired",
            )),
            _ => Ok(()),
        }
    }
}

pub struct QueryOptions<'a> {
    pub first: bool,
    pub unique_names: bool,
    pub deadline: Option<Instant>,
    pub cancellation: &'a Cancellation,
    pub cleaning: &'a dyn Fn() -> Result<bool, Error>,
}

pub(crate) fn deadline(timeout: Option<f64>) -> Result<Option<Instant>, Error> {
    deadline_from(Instant::now(), timeout)
}

pub(crate) fn deadline_from(
    start: Instant,
    timeout: Option<f64>,
) -> Result<Option<Instant>, Error> {
    timeout
        .map(|seconds| {
            let duration = Duration::try_from_secs_f64(seconds)
                .map_err(|_| Error::config("timeout", "requires finite nonnegative seconds"))?;
            start
                .checked_add(duration)
                .ok_or_else(|| Error::config("timeout", "duration overflow"))
        })
        .transpose()
}
