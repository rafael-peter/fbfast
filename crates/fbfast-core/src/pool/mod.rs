//! Bounded native workers. Construction performs no I/O and creates no threads.
mod cache_invalidation;
#[cfg(test)]
mod cache_tests;
mod controller;
mod delivery;
mod group;
mod maintenance;
mod pending;
mod query;
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
mod worker_finish;
mod worker_lifecycle;
mod worker_query;

use crate::{
    config::Config,
    error::{Error, ErrorKind},
    sinks::rows::Rows,
};
pub use delivery::DeliveryBudget;
pub use group::Signal;
pub use pending::Pending;
pub use query::{Query, Request, StreamFormat, StreamQuery};
pub use session::Session;
pub use state::Stats;
use state::{Inner, Work};
use std::{path::PathBuf, sync::Arc, time::Duration};
pub use stream::{Stream, StreamAction, StreamItem, StreamWait};

#[derive(Debug)]
pub enum Output {
    Arrow(crate::sinks::arrow::ArrowOutput),
    Rows(Rows),
    Json(crate::sinks::json::JsonOutput),
    Plan(String),
    Ready,
}
impl Output {
    pub(super) fn size(&self) -> Option<(u64, u64)> {
        match self {
            Self::Arrow(arrow) => Some((arrow.rows as u64, arrow.bytes as u64)),
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

    /// Queues one materialized query. SQL is validated (tokenized) here once.
    pub fn submit(&self, query: Query) -> Result<Pending, Error> {
        let inner = self.inner()?;
        query.validate()?;
        inner.submit(Work::Query(query))
    }

    pub fn warmup(&self) -> Result<Vec<Pending>, Error> {
        let inner = self.inner()?;
        inner.submit_warmup()
    }
    /// Starts a stream; batches flow with bounded prefetch as the consumer asks.
    pub fn open_stream(&self, query: StreamQuery) -> Result<Stream, Error> {
        let inner = self.inner()?;
        let batch_rows = query.validate(inner.config.fetch_size)?;
        let (sender, receiver) = flume::bounded(inner.config.prefetch_batches as usize + 1);
        let demand = Arc::new(stream_worker::Demand::default());
        let signal = Arc::new(Signal::default());
        let pending = inner.submit_signaled(
            Work::Stream {
                query,
                batch_rows,
                producer: stream_worker::Producer {
                    sender,
                    demand: demand.clone(),
                    signal: signal.clone(),
                },
            },
            Some(signal.clone()),
        )?;
        Ok(Stream::new(pending, receiver, demand, signal))
    }

    pub fn clear_statement_cache(&self, timeout: Option<f64>) -> Result<Pending, Error> {
        crate::execution::control::deadline(timeout)?;
        self.inner()?.request_cache_clear(timeout)
    }

    /// Maximum number of connections (`pool_size`).
    pub fn size(&self) -> Result<usize, Error> {
        Ok(self.inner()?.config.pool_size as usize)
    }

    pub fn stats(&self) -> Result<Stats, Error> {
        Ok(self.inner()?.stats())
    }

    /// Capabilities observed by the most recently opened attachment, if any.
    pub fn session_info(&self) -> Result<Option<crate::execution::SessionInfo>, Error> {
        let inner = self.inner()?;
        let state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
        Ok(state.session.clone())
    }

    /// Drains the per-operation records kept while `diagnostics` is enabled.
    pub fn diagnostics(&self) -> Result<Vec<crate::execution::QueryTrace>, Error> {
        let inner = self.inner()?;
        let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
        Ok(state.traces.drain(..).collect())
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
