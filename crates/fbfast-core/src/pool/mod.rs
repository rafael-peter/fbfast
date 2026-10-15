//! Bounded native workers. Construction performs no I/O and creates no threads.
mod cache_invalidation;
mod controller;
mod delivery;
mod maintenance;
mod pending;
mod scheduler;
mod state;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_live;
mod worker;
mod worker_lifecycle;

use crate::{
    config::Config,
    decode::Value,
    error::{Error, ErrorKind},
    sinks::rows::Rows,
};
pub use delivery::DeliveryBudget;
pub use pending::Pending;
pub use state::Stats;
use state::{Inner, Work};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Debug)]
pub enum Output {
    Rows(Rows),
    Plan(String),
    Ready,
}

#[derive(Clone)]
pub struct Pool {
    inner: Option<Arc<Inner>>,
    pid: u32,
}

impl Pool {
    pub fn new(config: Config, client_path: PathBuf) -> Self {
        Self {
            inner: Some(Arc::new(Inner::new(config, client_path))),
            pid: std::process::id(),
        }
    }

    fn inner(&self) -> Result<&Arc<Inner>, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        Ok(self.inner.as_ref().expect("pool inner exists until Drop"))
    }

    pub fn submit_rows(
        &self,
        sql: String,
        params: Vec<Value>,
        first: bool,
        unique_names: bool,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        crate::statements::validate_read_query(&sql)?;
        crate::execution::control::deadline(timeout)?;
        self.inner()?.submit(Work::Rows {
            sql,
            params,
            first,
            unique_names,
            timeout,
        })
    }

    pub fn submit_explain(
        &self,
        sql: String,
        params: Vec<Value>,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        crate::statements::validate_read_query(&sql)?;
        crate::execution::control::deadline(timeout)?;
        self.inner()?.submit(Work::Explain {
            sql,
            params,
            timeout,
        })
    }

    pub fn warmup(&self) -> Result<Vec<Pending>, Error> {
        let inner = self.inner()?;
        inner.submit_warmup()
    }
    pub fn clear_statement_cache(&self, timeout: Option<f64>) -> Result<Pending, Error> {
        crate::execution::control::deadline(timeout)?;
        self.inner()?.request_cache_clear(timeout)
    }

    pub fn stats(&self) -> Result<Stats, Error> {
        Ok(self.inner()?.stats())
    }

    pub fn begin_close(&self) -> Result<(), Error> {
        self.inner()?.begin_close();
        Ok(())
    }

    /// A bounded wait; native work remains counted if it cannot finish in time.
    pub fn close(&self, timeout: Duration) -> Result<(), Error> {
        let inner = self.inner()?;
        inner.begin_close();
        inner.wait_closed(timeout)
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        if self.pid != std::process::id() {
            // Never drop inherited mutexes/channels or native handles in a fork child.
            if let Some(inner) = self.inner.take() {
                std::mem::forget(inner);
            }
        }
    }
}

fn fork_error() -> Error {
    Error::new(
        ErrorKind::ForkSafety,
        "lifecycle",
        "Pool belongs to another process; initialize after fork",
    )
}
