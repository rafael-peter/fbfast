use super::{
    state::{Command, Inner, Job, Phase, State, Work},
    worker, Pending,
};
use crate::error::{Error, ErrorKind};
use std::{
    sync::{mpsc::sync_channel, Arc},
    time::{Duration, Instant},
};

impl Inner {
    pub fn submit(self: &Arc<Self>, work: Work) -> Result<Pending, Error> {
        self.submit_many(vec![work])
            .map(|mut requests| requests.remove(0))
    }

    pub fn submit_warmup(self: &Arc<Self>) -> Result<Vec<Pending>, Error> {
        self.submit_many(
            (0..self.config.min_idle.max(1))
                .map(|_| Work::Warmup)
                .collect(),
        )
    }

    fn submit_many(self: &Arc<Self>, work: Vec<Work>) -> Result<Vec<Pending>, Error> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closing {
            return Err(closed());
        }
        if state.next_id.checked_add(work.len() as u64).is_none() {
            return Err(Error::new(
                ErrorKind::PoolClosed,
                "acquire",
                "Operation generations exhausted",
            ));
        }
        let available = state
            .slots
            .iter()
            .filter(|slot| matches!(slot.phase, Phase::Idle | Phase::Dormant | Phase::Closed))
            .count();
        if (state.queue.len() + work.len()).saturating_sub(available) + state.cache_waiters.len()
            > self.config.max_waiters as usize
        {
            return Err(Error::new(
                ErrorKind::PoolBusy,
                "acquire",
                "Pool waiting queue is full",
            ));
        }
        let mut requests = Vec::with_capacity(work.len());
        state.initialized = true;
        for work in work {
            let (job, pending) = self.job(&mut state, work);
            state.queue.push_back(job);
            requests.push(pending);
        }
        self.dispatch(&mut state);
        Ok(requests)
    }

    pub fn dispatch(self: &Arc<Self>, state: &mut State) {
        while !state.queue.is_empty() && !state.closing {
            let idle = state
                .slots
                .iter()
                .enumerate()
                .filter(|(_, s)| s.phase == Phase::Idle)
                .max_by_key(|(_, s)| s.idle_since)
                .map(|(i, _)| i);
            let Some(index) = idle.or_else(|| {
                state
                    .slots
                    .iter()
                    .position(|s| matches!(s.phase, Phase::Dormant | Phase::Closed))
            }) else {
                break;
            };
            let job = state.queue.pop_front().expect("queue not empty");
            if job.progress.cancellation.check().is_err() {
                continue;
            }
            if Instant::now() >= job.acquire_deadline {
                state.stats.timeouts += 1;
                let _ = job.reply.send(Err(Error::new(
                    ErrorKind::PoolTimeout,
                    "acquire",
                    "Pool acquisition deadline expired",
                )));
                continue;
            }
            self.start_slot(state, index, job);
        }
        self.keep_min_idle(state);
    }

    pub(super) fn start_slot(self: &Arc<Self>, state: &mut State, index: usize, job: Job) -> bool {
        if state.slots[index].phase == Phase::Closed {
            let (sender, receiver) = sync_channel(1);
            state.slots[index].sender = Some(sender);
            state.slots[index].phase = Phase::Opening;
            let weak = Arc::downgrade(self);
            let config = self.config.clone();
            let client = self.client.clone();
            if let Err(error) = std::thread::Builder::new()
                .name(format!("fbfast-{index}"))
                .spawn(move || worker::run(weak, index, receiver, config, client))
            {
                state.slots[index].phase = Phase::Closed;
                state.slots[index].sender = None;
                let _ = job.reply.send(Err(Error::new(
                    ErrorKind::PoolBusy,
                    "worker",
                    format!("Cannot start native worker: {error}"),
                )));
                return false;
            }
        } else {
            state.slots[index].phase = if state.slots[index].phase == Phase::Dormant {
                Phase::Opening
            } else {
                Phase::Leased
            };
        }
        // At most one job targets a reserved slot; never wait on native I/O.
        if let Some(sender) = &state.slots[index].sender {
            if let Err(error) = sender.try_send(Command::Job(job)) {
                let (std::sync::mpsc::TrySendError::Full(command)
                | std::sync::mpsc::TrySendError::Disconnected(command)) = error;
                if let Command::Job(job) = command {
                    let _ = job.reply.send(Err(Error::new(
                        ErrorKind::PoolClosed,
                        "worker",
                        "Native worker is unavailable",
                    )));
                }
                return false;
            }
        }
        true
    }

    pub fn begin_close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closing = true;
        for job in state.queue.drain(..) {
            let _ = job.reply.send(Err(closed()));
        }
        for waiter in state.cache_waiters.drain(..) {
            let _ = waiter.job.reply.send(Err(closed()));
        }
        for slot in &mut state.slots {
            if matches!(slot.phase, Phase::Idle | Phase::Dormant) {
                slot.phase = Phase::Closing;
                if let Some(sender) = &slot.sender {
                    let _ = sender.try_send(Command::Close);
                }
            }
        }
        self.changed.notify_all();
    }

    pub fn wait_closed(&self, timeout: Duration) -> Result<(), Error> {
        let end = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| Error::config("close_timeout", "duration overflow"))?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while state.slots.iter().any(|s| s.phase != Phase::Closed) {
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::new(
                    ErrorKind::PoolTimeout,
                    "close",
                    "Shutdown deadline expired; native slots remain counted until cleanup finishes",
                ));
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        if !state.close_errors.is_empty() {
            let mut error = Error::new(
                ErrorKind::Cleanup,
                "close",
                "Pool closed with cleanup errors",
            );
            error.cleanup = std::mem::take(&mut state.close_errors);
            return Err(error);
        }
        Ok(())
    }
}

fn closed() -> Error {
    Error::new(
        ErrorKind::PoolClosed,
        "acquire",
        "Pool is closing or closed",
    )
}
