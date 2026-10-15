use super::{
    controller::Controller,
    state::{Command, Inner, Job, Phase, Work},
    worker_lifecycle::{acquire_timeout, close_connection, phase, retire_idle},
    Output,
};
use crate::{
    config::Config,
    error::{Error, ErrorKind},
    execution::{
        control::{deadline_from, QueryOptions},
        ExecutionConnection,
    },
};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{Receiver, RecvTimeoutError},
        Weak,
    },
    time::{Duration, Instant},
};

pub(super) fn run(
    inner: Weak<Inner>,
    index: usize,
    receiver: Receiver<Command>,
    config: Config,
    client: PathBuf,
) {
    let mut connection: Option<ExecutionConnection> = None;
    let mut born = Instant::now();
    let mut last_used = born;
    let mut lifetime = config.max_lifetime;
    let mut failures = 0u32;
    let mut controller = None;
    loop {
        let command = match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(command) => command,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if retire_idle(&inner, index, &config, last_used, born, lifetime) {
                    break;
                }
                continue;
            }
        };
        let job = match command {
            Command::Job(job) => job,
            Command::ClearCache => {
                if super::worker_lifecycle::idle_cache(&inner, index, &mut connection) {
                    break;
                }
                continue;
            }
            Command::Close => break,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job.progress.cancellation.check()?;
            if let Some(inner) = inner.upgrade() {
                if inner
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .closing
                {
                    return Err(Error::new(
                        ErrorKind::PoolClosed,
                        "acquire",
                        "Pool closed during acquisition",
                    ));
                }
            } else {
                return Err(Error::new(
                    ErrorKind::PoolClosed,
                    "acquire",
                    "Pool was dropped",
                ));
            }
            if connection.is_none() {
                phase(&inner, index, Phase::Opening);
                if failures > 0 {
                    // Per-slot bounded backoff with deterministic independent jitter; no query replay.
                    let millis =
                        (50u64 << failures.min(5)) + ((index as u64 * 37 + job.id * 17) % 53);
                    std::thread::sleep(Duration::from_millis(millis));
                }
                if Instant::now() >= job.acquire_deadline {
                    return Err(acquire_timeout());
                }
                connection = Some(ExecutionConnection::connect(config.clone(), &client)?);
                born = Instant::now();
                lifetime = config.max_lifetime
                    * (0.9 + ((index as u64 * 37 + job.id % 100) % 100) as f64 / 1000.0);
                failures = 0;
                if let Some(inner) = inner.upgrade() {
                    inner
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .stats
                        .connections_created += 1;
                }
            }
            let connection = connection.as_mut().expect("opened connection");
            if last_used.elapsed().as_secs_f64() >= config.health_check_after {
                connection.ping()?;
            }
            job.progress.cancellation.check()?;
            if Instant::now() >= job.acquire_deadline {
                return Err(acquire_timeout());
            }
            if controller.is_none() {
                controller = Some(Controller::new(inner.clone(), index)?);
            }
            let started = Instant::now();
            let end = deadline_from(started, job.work.timeout())?;
            *job.progress
                .started
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(started);
            phase(&inner, index, Phase::Leased);
            if let Some(inner) = inner
                .upgrade()
                .filter(|_| !matches!(job.work, Work::KeepWarm))
            {
                inner
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .stats
                    .acquires += 1;
            }
            let control = connection.begin_control(job.id)?;
            let lease = controller.as_ref().expect("controller initialized").arm(
                control,
                job.id,
                end,
                job.progress.clone(),
            );
            let mut result = execute(connection, &job, end, &|| {
                phase(&inner, index, Phase::Cleaning);
                lease.finish()
            });
            // Also disarm for validation/begin failures that precede query cleanup.
            if !lease.is_finished() {
                match lease.finish() {
                    Ok(true) => connection.poison(),
                    Err(cleanup) => {
                        connection.poison();
                        match &mut result {
                            Err(error) => error.cleanup.push(cleanup),
                            Ok(_) => result = Err(cleanup),
                        }
                    }
                    Ok(false) => (),
                }
            }
            result
        }));
        let panicked = result.is_err();
        let mut result = result.unwrap_or_else(|_| {
            if let Some(connection) = &mut connection {
                connection.poison();
            }
            Err(Error::new(
                ErrorKind::Query,
                "worker",
                "Native worker panicked; attachment is being discarded",
            ))
        });
        if let Err(cleanup) = super::cache_invalidation::service(&inner, index, &mut connection) {
            match &mut result {
                Err(error) => error.cleanup.push(cleanup),
                Ok(_) => result = Err(cleanup),
            }
        }
        if connection.is_none() && result.is_err() {
            failures = failures.saturating_add(1);
        }
        let closing = panicked
            || inner.upgrade().is_none_or(|inner| {
                inner
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .closing
            });
        let expired = lifetime > 0.0 && born.elapsed().as_secs_f64() >= lifetime;
        let unhealthy = connection.as_ref().is_some_and(|c| !c.is_healthy());
        if closing || expired || unhealthy {
            phase(&inner, index, Phase::Closing);
            let cleanup = close_connection(&inner, &mut connection);
            super::cache_invalidation::closed(&inner, index, cleanup.as_ref().err());
            if let Err(cleanup) = cleanup {
                match &mut result {
                    Err(error) => error.cleanup.push(cleanup),
                    Ok(_) => result = Err(cleanup),
                }
            }
        }
        let native_timeout = result
            .as_ref()
            .is_err_and(|e| e.kind == ErrorKind::QueryTimeout)
            && job.progress.cancellation.timeout();
        if let Some(inner) = inner.upgrade() {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(job.work, Work::KeepWarm) && result.is_err() {
                state.stats.maintenance_failures += 1;
            }
            // The server's own statement timeout can win the controller race.
            if native_timeout {
                state.stats.timeouts += 1;
            }
            if unhealthy {
                state.stats.discarded += 1;
            }
            if let Ok(Output::Rows(rows)) = &result {
                state.stats.rows += rows.count as u64;
                state.stats.result_bytes += rows.bytes as u64;
            }
            if let Some(connection) = &mut connection {
                let (hits, misses) = connection.take_cache_counts();
                state.stats.statement_hits += hits;
                state.stats.statement_misses += misses;
            }
            last_used = Instant::now();
            if !closing {
                super::cache_invalidation::available(&mut state, index, connection.is_some());
                state.slots[index].idle_since = last_used;
                inner.dispatch(&mut state);
            }
            inner.changed.notify_all();
        }
        let _ = job.reply.send(result);
        if closing {
            break;
        }
    }
    let cleanup = close_connection(&inner, &mut connection);
    super::cache_invalidation::closed(&inner, index, cleanup.as_ref().err());
    let control_cleanup = controller.map(Controller::stop_and_join).transpose();
    if let Some(inner) = inner.upgrade() {
        let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(error) = cleanup {
            state.close_errors.push(error);
            state.stats.cleanup_errors += 1;
        }
        if let Err(error) = control_cleanup {
            state.close_errors.push(error);
            state.stats.cleanup_errors += 1;
        }
        state.slots[index].sender = None;
        state.slots[index].phase = Phase::Closed;
        super::cache_invalidation::empty(&mut state, index);
        inner.dispatch(&mut state);
        inner.changed.notify_all();
    }
}

fn execute(
    connection: &mut ExecutionConnection,
    job: &Job,
    end: Option<Instant>,
    cleaning: &dyn Fn() -> Result<bool, Error>,
) -> Result<Output, Error> {
    let options = |first, unique_names| QueryOptions {
        first,
        unique_names,
        deadline: end,
        cancellation: &job.progress.cancellation,
        cleaning,
    };
    match &job.work {
        Work::Rows {
            sql,
            params,
            first,
            unique_names,
            ..
        } => connection
            .fetch_with(sql, params, &options(*first, *unique_names))
            .map(Output::Rows),
        Work::Explain { sql, params, .. } => connection
            .explain_with(sql, params, &options(true, false))
            .map(Output::Plan),
        Work::Warmup | Work::KeepWarm => Ok(Output::Ready),
        Work::ClearCache { .. } => Err(Error::new(
            ErrorKind::Query,
            "cache_clear",
            "Cache invalidation must use the coordinated path",
        )),
        #[cfg(test)]
        Work::Panic => panic!("intentional worker fault for lifecycle test"),
    }
}
