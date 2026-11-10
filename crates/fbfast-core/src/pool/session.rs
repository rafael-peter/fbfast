//! One reserved worker per snapshot. There is no independent session worker pool.
use super::{
    fork_error,
    pending::Completion,
    session_scope::{self, Scope},
    state::{Job, Work},
    stream_worker::{Demand, Producer},
    Output, Pending, Pool, Query, Stats, Stream, StreamQuery,
};
use crate::error::{Error, ErrorKind};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

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
                completion: Mutex::new(Completion::new(pending)),
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
    fn enqueue(&self, work: Work) -> Result<Pending, Error> {
        self.enqueue_signaled(work, None)
    }
    fn enqueue_signaled(
        &self,
        work: Work,
        signal: Option<Arc<super::Signal>>,
    ) -> Result<Pending, Error> {
        let h = self.handle()?;
        let inner = h.pool.inner()?;
        let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closing {
            return Err(session_scope::closed());
        }
        if state.next_id == u64::MAX {
            return Err(session_scope::closed());
        }
        let (job, mut pending) = inner.job_signaled(&mut state, work, signal);
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
    /// Queues one query on the snapshot's reserved connection.
    pub fn submit(&self, query: Query) -> Result<Pending, Error> {
        self.handle()?;
        query.validate()?;
        self.enqueue(Work::Query(query))
    }

    pub fn open_stream(&self, query: StreamQuery) -> Result<Stream, Error> {
        let h = self.handle()?;
        let config = &h.pool.inner()?.config;
        let batch_rows = query.validate(config.fetch_size)?;
        let (sender, receiver) = flume::bounded(config.prefetch_batches as usize + 1);
        let demand = Arc::new(Demand::default());
        let signal = Arc::new(super::Signal::default());
        let pending = self.enqueue_signaled(
            Work::Stream {
                query,
                batch_rows,
                producer: Producer {
                    sender,
                    demand: demand.clone(),
                    signal: signal.clone(),
                },
            },
            Some(signal.clone()),
        )?;
        Ok(Stream::new(pending, receiver, demand, signal))
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
            let remaining = end.saturating_duration_since(Instant::now());
            let outcome = h
                .completion
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .wait(remaining.min(Duration::from_millis(50)), "snapshot");
            if let Some(result) = outcome {
                return result;
            }
            if Instant::now() >= end {
                return Err(Error::new(
                    ErrorKind::PoolTimeout,
                    "snapshot_close",
                    "Snapshot cleanup pending; native slot remains counted",
                ));
            }
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
