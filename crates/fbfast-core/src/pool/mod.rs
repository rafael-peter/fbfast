//! Bounded native workers. Construction performs no I/O and creates no threads.
mod cache_invalidation;
#[cfg(test)]
mod cache_tests;
mod controller;
mod delivery;
mod maintenance;
mod pending;
mod scheduler;
mod session;
mod session_scope;
mod session_worker;
mod state;
mod stream;
mod stream_worker;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_live;
mod worker;
mod worker_lifecycle;
mod worker_query;

use crate::{
    config::Config,
    decode::Value,
    error::{Error, ErrorKind},
    sinks::rows::Rows,
};
pub use delivery::DeliveryBudget;
pub use pending::Pending;
pub use session::Session;
pub use state::Stats;
use state::{Inner, Work};
use std::{path::PathBuf, sync::Arc, time::Duration};
pub use stream::{Stream, StreamAction, StreamItem, StreamWait};

#[derive(Debug)]
pub enum Output {
    Rows(Rows),
    Json(crate::sinks::json::JsonOutput),
    Plan(String),
    Ready,
}
impl Output {
    pub(super) fn size(&self) -> Option<(u64, u64)> {
        match self {
            Self::Rows(rows) => Some((rows.count as u64, rows.bytes as u64)),
            Self::Json(json) => Some((json.rows as u64, json.data.len() as u64)),
            _ => None,
        }
    }
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

    pub fn submit_json(
        &self,
        sql: String,
        params: Vec<Value>,
        json: crate::sinks::json::JsonOptions,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        let inner = self.inner()?;
        crate::statements::validate_read_query(&sql)?;
        crate::execution::control::deadline(timeout)?;
        inner.submit(Work::Json {
            sql,
            params,
            json,
            timeout,
        })
    }

    pub fn warmup(&self) -> Result<Vec<Pending>, Error> {
        let inner = self.inner()?;
        inner.submit_warmup()
    }
    pub fn stream(
        &self,
        sql: String,
        params: Vec<Value>,
        batch_rows: Option<u32>,
        unique_names: bool,
        timeout: Option<f64>,
    ) -> Result<Stream, Error> {
        self.stream_output(sql, params, batch_rows, unique_names, timeout, None)
    }
    pub fn stream_json(
        &self,
        sql: String,
        params: Vec<Value>,
        batch_rows: Option<u32>,
        json: (
            crate::sinks::json::JsonOptions,
            crate::sinks::json::JsonFormat,
        ),
        timeout: Option<f64>,
    ) -> Result<Stream, Error> {
        self.stream_output(sql, params, batch_rows, json.0.records, timeout, Some(json))
    }
    fn stream_output(
        &self,
        sql: String,
        params: Vec<Value>,
        batch_rows: Option<u32>,
        unique_names: bool,
        timeout: Option<f64>,
        json: Option<(
            crate::sinks::json::JsonOptions,
            crate::sinks::json::JsonFormat,
        )>,
    ) -> Result<Stream, Error> {
        crate::statements::validate_read_query(&sql)?;
        crate::execution::control::deadline(timeout)?;
        let inner = self.inner()?;
        let batch_rows = batch_rows.unwrap_or(inner.config.fetch_size);
        if batch_rows == 0 {
            return Err(Error::config("batch_rows", "must be positive"));
        }
        let (sender, receiver) = flume::bounded(inner.config.prefetch_batches as usize + 1);
        let demand = Arc::new(stream_worker::Demand::default());
        let pending = inner.submit(Work::Stream {
            sql,
            params,
            batch_rows,
            unique_names,
            json,
            timeout,
            producer: stream_worker::Producer {
                sender,
                demand: demand.clone(),
            },
        })?;
        Ok(Stream::new(pending, receiver, demand))
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
