//! One reserved worker per snapshot. There is no independent session worker pool.
use super::{
    fork_error,
    session_scope::{self, Scope},
    state::{Job, Work},
    stream_worker::{Demand, Producer},
    Output, Pending, Pool, Stats, Stream,
};
use crate::{
    decode::Value,
    error::{Error, ErrorKind},
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

struct Completion {
    pending: Pending,
    result: Option<Result<(), Error>>,
}
struct Handle {
    pool: Pool,
    scope: Arc<Scope>,
    requests: flume::Sender<Job>,
    opened: flume::Receiver<Result<Output, Error>>,
    entered: AtomicBool,
    completion: Mutex<Completion>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.scope.abort();
    }
}

#[derive(Clone)]
pub struct Session {
    handle: Option<Arc<Handle>>,
    pid: u32,
}

impl Pool {
    pub fn snapshot(&self, timeout: Option<f64>) -> Result<Session, Error> {
        crate::execution::control::deadline(timeout)?;
        let inner = self.inner()?;
        let scope = Scope::new(Arc::downgrade(inner));
        let (sender, requests) = flume::bounded(1);
        let (opened, receiver) = flume::bounded(1);
        let pending = inner.submit(Work::Snapshot {
            scope: scope.clone(),
            requests,
            opened,
            timeout,
        })?;
        Ok(Session {
            handle: Some(Arc::new(Handle {
                pool: self.clone(),
                scope,
                requests: sender,
                opened: receiver,
                entered: AtomicBool::new(false),
                completion: Mutex::new(Completion {
                    pending,
                    result: None,
                }),
            })),
            pid: std::process::id(),
        })
    }
}

impl Session {
    fn handle(&self) -> Result<&Arc<Handle>, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        Ok(self.handle.as_ref().expect("session exists until Drop"))
    }
    pub fn open(&self) -> Result<Pending, Error> {
        let h = self.handle()?;
        if h.entered.swap(true, Ordering::AcqRel) {
            return Err(Error::new(
                ErrorKind::Query,
                "snapshot",
                "Enter a snapshot exactly once",
            ));
        }
        Ok(h.completion
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .opening_receiver(h.opened.clone()))
    }
    pub fn stats(&self) -> Result<Stats, Error> {
        self.handle()?.pool.stats()
    }
    fn submit(&self, work: Work) -> Result<Pending, Error> {
        let h = self.handle()?;
        let inner = h.pool.inner()?;
        let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closing {
            return Err(session_scope::closed());
        }
        if state.next_id == u64::MAX {
            return Err(session_scope::closed());
        }
        let (job, mut pending) = inner.job(&mut state, work);
        drop(state);
        pending.scope = match h.scope.reserve(job.progress.clone()) {
            Ok(permit) => Some(permit),
            Err(error) => {
                pending.detach();
                return Err(error);
            }
        };
        if h.requests.try_send(job).is_err() {
            return Err(session_scope::closed());
        }
        Ok(pending)
    }
    pub fn submit_rows(
        &self,
        sql: String,
        params: Vec<Value>,
        first: bool,
        unique_names: bool,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        self.handle()?;
        crate::statements::validate_read_query(&sql)?;
        crate::execution::control::deadline(timeout)?;
        self.submit(Work::Rows {
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
        self.handle()?;
        crate::statements::validate_read_query(&sql)?;
        crate::execution::control::deadline(timeout)?;
        self.submit(Work::Explain {
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
        self.handle()?;
        crate::statements::validate_read_query(&sql)?;
        crate::execution::control::deadline(timeout)?;
        self.submit(Work::Json {
            sql,
            params,
            json,
            timeout,
        })
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
        let h = self.handle()?;
        crate::statements::validate_read_query(&sql)?;
        crate::execution::control::deadline(timeout)?;
        let config = &h.pool.inner()?.config;
        let batch_rows = batch_rows.unwrap_or(config.fetch_size);
        if batch_rows == 0 {
            return Err(Error::config("batch_rows", "must be positive"));
        }
        let (sender, receiver) = flume::bounded(config.prefetch_batches as usize + 1);
        let demand = Arc::new(Demand::default());
        let pending = self.submit(Work::Stream {
            sql,
            params,
            batch_rows,
            unique_names,
            json,
            timeout,
            producer: Producer {
                sender,
                demand: demand.clone(),
            },
        })?;
        Ok(Stream::new(pending, receiver, demand))
    }
    pub fn begin_close(&self) -> Result<(), Error> {
        self.handle()?.scope.request_close();
        Ok(())
    }
    pub fn close(&self, timeout: Duration) -> Result<(), Error> {
        self.begin_close()?;
        let h = self.handle()?;
        let end = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| Error::config("close_timeout", "duration overflow"))?;
        loop {
            let mut completion = h.completion.lock().unwrap_or_else(|e| e.into_inner());
            if completion.result.is_none() {
                completion.result = match completion.pending.try_result() {
                    Ok(None) => None,
                    Ok(Some(Output::Ready)) => Some(Ok(())),
                    Ok(Some(_)) => Some(Err(Error::new(
                        ErrorKind::Query,
                        "snapshot",
                        "Unexpected session completion",
                    ))),
                    Err(error) => Some(Err(error)),
                };
            }
            if let Some(result) = &completion.result {
                return result.clone();
            }
            drop(completion);
            if Instant::now() >= end {
                return Err(Error::new(
                    ErrorKind::PoolTimeout,
                    "snapshot_close",
                    "Snapshot cleanup pending; native slot remains counted",
                ));
            }
            std::thread::sleep(
                end.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(5)),
            );
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.pid != std::process::id() {
            if let Some(handle) = self.handle.take() {
                std::mem::forget(handle);
            }
        }
    }
}
