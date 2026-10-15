//! Runs inside the already-reserved native worker; commands have capacity one.
use super::{
    controller::Controller,
    state::{Inner, Job, Phase, Work},
    worker_lifecycle::phase,
    Output,
};
use crate::{
    error::{Error, ErrorKind},
    execution::{
        control::{deadline_from, QueryOptions},
        ExecutionConnection,
    },
};
use std::{
    sync::{atomic::Ordering, Weak},
    time::{Duration, Instant},
};

pub(super) fn run(
    inner: &Weak<Inner>,
    index: usize,
    connection: &mut ExecutionConnection,
    job: &Job,
    controller: &Controller,
    end: Option<Instant>,
) -> Result<Output, Error> {
    let Work::Snapshot {
        scope,
        requests: _,
        opened,
        timeout,
    } = &job.work
    else {
        unreachable!()
    };
    *scope.deadline.lock().unwrap_or_else(|e| e.into_inner()) = end;
    let control = connection.begin_control(job.id)?;
    let lease = controller.arm(control, job.id, end, job.progress.clone());
    let begin = connection.begin_snapshot(*timeout);
    let disarm = lease.finish();
    drop(lease);
    scope.forward_cancellation();
    if disarm.as_ref().is_ok_and(|raised| *raised) || disarm.is_err() {
        connection.poison();
    }
    let mut result = match (begin, disarm) {
        (Err(mut error), Err(cleanup)) => {
            error.cleanup.push(cleanup);
            Err(error)
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(()), Ok(_)) => scope.check(),
    };
    count_timeout(inner, job, &result);
    if result.is_ok() {
        scope.ready.store(true, Ordering::Release);
        if opened.try_send(Ok(Output::Ready)).is_err() {
            scope.abort();
        }
        result = operations(inner, index, connection, job, controller, end);
    }
    scope.closing.store(true, Ordering::Release);
    if let Err(error) = &result {
        *scope.failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
    }
    if let Work::Snapshot { requests, .. } = &job.work {
        for request in requests.try_iter() {
            let error = result
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(super::session_scope::closed);
            let _ = request.reply.try_send(Err(error));
        }
    }
    // Child/controller/delivery or the idle scope check has accounted this event.
    // Mark the parent so the outer worker does not count the same timeout again.
    if result
        .as_ref()
        .is_err_and(|e| e.kind == ErrorKind::QueryTimeout)
    {
        job.progress.cancellation.timeout();
    }
    phase(inner, index, Phase::Cleaning);
    let cleanup = connection.finish_snapshot(&QueryOptions {
        first: true,
        unique_names: false,
        deadline: end,
        cancellation: &job.progress.cancellation,
        cleaning: &|| Ok(false),
    });
    match (result, cleanup) {
        (Err(mut error), Err(cleanup)) => {
            error.cleanup.push(cleanup);
            Err(error)
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(Output::Ready),
    }
}

fn operations(
    inner: &Weak<Inner>,
    index: usize,
    connection: &mut ExecutionConnection,
    parent: &Job,
    controller: &Controller,
    end: Option<Instant>,
) -> Result<(), Error> {
    let Work::Snapshot {
        scope, requests, ..
    } = &parent.work
    else {
        unreachable!()
    };
    loop {
        if scope.closing.load(Ordering::Acquire) {
            return Ok(());
        }
        scope.check()?;
        scope.check_active()?;
        if inner.upgrade().is_none_or(|owner| {
            owner
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .closing
        }) {
            return Err(Error::new(
                ErrorKind::PoolClosed,
                "snapshot",
                "Pool closed during snapshot",
            ));
        }
        let job = match requests.recv_timeout(Duration::from_millis(10)) {
            Ok(job) => job,
            Err(flume::RecvTimeoutError::Timeout) => continue,
            Err(flume::RecvTimeoutError::Disconnected) => return Ok(()),
        };
        if scope.closing.load(Ordering::Acquire) {
            let _ = job.reply.try_send(Err(super::session_scope::closed()));
            return Ok(());
        }
        if let Err(error) = scope.check() {
            let _ = job.reply.try_send(Err(error.clone()));
            return Err(error);
        }
        let result = operation(inner, index, connection, &job, controller, end);
        let failed = result.as_ref().err().cloned();
        if let Some((rows, bytes)) = result.as_ref().ok().and_then(Output::size) {
            if let Some(owner) = inner.upgrade() {
                let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
                state.stats.rows += rows;
                state.stats.result_bytes += bytes;
            }
        }
        let _ = job.reply.send(result);
        if let Some(error) = failed {
            return Err(error);
        }
    }
}

fn operation(
    inner: &Weak<Inner>,
    index: usize,
    connection: &mut ExecutionConnection,
    job: &Job,
    controller: &Controller,
    snapshot_end: Option<Instant>,
) -> Result<Output, Error> {
    job.progress.cancellation.check()?;
    let started = Instant::now();
    *job.progress
        .started
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(started);
    let requested = deadline_from(started, job.work.timeout())?;
    let end = match (requested, snapshot_end) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let control = connection.begin_control(job.id)?;
    let lease = controller.arm(control, job.id, end, job.progress.clone());
    let mut result = super::worker_query::execute(inner, connection, job, end, &|| lease.finish());
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
    phase(inner, index, Phase::Leased);
    count_timeout(inner, job, &result);
    result
}

fn count_timeout<T>(inner: &Weak<Inner>, job: &Job, result: &Result<T, Error>) {
    if result
        .as_ref()
        .is_err_and(|e| e.kind == ErrorKind::QueryTimeout)
        && job.progress.cancellation.timeout()
    {
        if let Some(inner) = inner.upgrade() {
            inner
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .stats
                .timeouts += 1;
        }
    }
}
