use super::{Output, Pending};
use crate::{config::Config, decode::Value, error::Error, execution::control::Cancellation};
use serde::Serialize;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{mpsc::SyncSender, Arc, Condvar, Mutex},
    time::Instant,
};

pub(super) enum Work {
    Snapshot {
        scope: Arc<super::session_scope::Scope>,
        requests: flume::Receiver<Job>,
        opened: flume::Sender<Result<Output, Error>>,
        timeout: Option<f64>,
    },
    Stream {
        sql: String,
        params: Vec<Value>,
        batch_rows: u32,
        unique_names: bool,
        json: Option<(
            crate::sinks::json::JsonOptions,
            crate::sinks::json::JsonFormat,
        )>,
        timeout: Option<f64>,
        producer: super::stream_worker::Producer,
    },
    Json {
        sql: String,
        params: Vec<Value>,
        json: crate::sinks::json::JsonOptions,
        timeout: Option<f64>,
    },
    Rows {
        sql: String,
        params: Vec<Value>,
        first: bool,
        unique_names: bool,
        timeout: Option<f64>,
    },
    Explain {
        sql: String,
        params: Vec<Value>,
        timeout: Option<f64>,
    },
    Warmup,
    KeepWarm,
    ClearCache {
        timeout: Option<f64>,
    },
    #[cfg(test)]
    Panic,
}

impl Work {
    pub fn timeout(&self) -> Option<f64> {
        match self {
            Self::Rows { timeout, .. }
            | Self::Json { timeout, .. }
            | Self::Snapshot { timeout, .. }
            | Self::Stream { timeout, .. }
            | Self::Explain { timeout, .. }
            | Self::ClearCache { timeout } => *timeout,
            _ => None,
        }
    }
}

#[derive(Default)]
pub(super) struct Progress {
    pub cancellation: Cancellation,
    pub started: Mutex<Option<Instant>>,
}

pub(super) struct Job {
    pub id: u64,
    pub work: Work,
    pub progress: Arc<Progress>,
    pub reply: flume::Sender<Result<Output, Error>>,
    pub acquire_deadline: Instant,
}

pub(super) enum Command {
    Job(Job),
    ClearCache,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Closed,
    Dormant,
    Opening,
    Idle,
    Leased,
    Cleaning,
    Closing,
}

pub(super) struct Slot {
    pub cache_epoch: u64,
    pub phase: Phase,
    pub sender: Option<SyncSender<Command>>,
    pub idle_since: Instant,
}

#[derive(Default, Clone, Serialize)]
pub struct Stats {
    pub cache_invalidations: u64,
    pub maintenance_failures: u64,
    pub control_threads: usize,
    pub native_cancels: u64,
    pub dormant: usize,
    pub opening: usize,
    pub idle: usize,
    pub leased: usize,
    pub cleaning: usize,
    pub closing: usize,
    pub closed: usize,
    pub waiting: usize,
    pub acquires: u64,
    pub timeouts: u64,
    pub cancellations: u64,
    pub discarded: u64,
    pub connections_created: u64,
    pub statement_hits: u64,
    pub statement_misses: u64,
    pub rows: u64,
    pub result_bytes: u64,
    pub closing_pool: bool,
    pub cleanup_errors: u64,
}

pub(super) struct State {
    pub cache_epoch: u64,
    pub cache_waiters: Vec<super::cache_invalidation::Waiter>,
    pub slots: Vec<Slot>,
    pub queue: VecDeque<Job>,
    pub next_id: u64,
    pub closing: bool,
    pub initialized: bool,
    pub stats: Stats,
    pub close_errors: Vec<Error>,
}

pub(super) struct Inner {
    pub config: Config,
    pub client: PathBuf,
    pub state: Mutex<State>,
    pub changed: Condvar,
}

impl Inner {
    pub fn new(config: Config, client: PathBuf) -> Self {
        let slots = (0..config.pool_size)
            .map(|_| Slot {
                cache_epoch: 0,
                phase: Phase::Closed,
                sender: None,
                idle_since: Instant::now(),
            })
            .collect();
        Self {
            config,
            client,
            state: Mutex::new(State {
                cache_epoch: 0,
                cache_waiters: Vec::new(),
                slots,
                queue: VecDeque::new(),
                next_id: 0,
                closing: false,
                initialized: false,
                stats: Stats::default(),
                close_errors: Vec::new(),
            }),
            changed: Condvar::new(),
        }
    }

    pub fn job(self: &Arc<Self>, state: &mut State, work: Work) -> (Job, Pending) {
        state.next_id += 1;
        let progress = match &work {
            Work::Snapshot { scope, .. } => scope.progress.clone(),
            _ => Arc::new(Progress::default()),
        };
        let (reply, receiver) = flume::bounded(1);
        // Config resolution already verifies finite, positive acquire_timeout.
        let acquire_deadline =
            Instant::now() + std::time::Duration::from_secs_f64(self.config.acquire_timeout);
        let pending = Pending::new(
            self,
            state.next_id,
            progress.clone(),
            receiver,
            acquire_deadline,
            work.timeout(),
        );
        (
            Job {
                id: state.next_id,
                work,
                progress,
                reply,
                acquire_deadline,
            },
            pending,
        )
    }

    pub fn stats(&self) -> Stats {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut stats = state.stats.clone();
        for slot in &state.slots {
            match slot.phase {
                Phase::Dormant => stats.dormant += 1,
                Phase::Closed => stats.closed += 1,
                Phase::Opening => stats.opening += 1,
                Phase::Idle => stats.idle += 1,
                Phase::Leased => stats.leased += 1,
                Phase::Cleaning => stats.cleaning += 1,
                Phase::Closing => stats.closing += 1,
            }
        }
        stats.waiting = state.queue.len() + state.cache_waiters.len();
        stats.closing_pool = state.closing;
        stats
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Only CPU/channel work here; workers own explicit protocol cleanup.
        let state = self.state.get_mut().unwrap_or_else(|e| e.into_inner());
        for slot in &state.slots {
            if let Some(sender) = &slot.sender {
                let _ = sender.try_send(Command::Close);
            }
        }
    }
}
