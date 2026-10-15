use super::state::{Inner, Phase};
use crate::{
    config::Config,
    error::{Error, ErrorKind},
    execution::ExecutionConnection,
};
use std::{sync::Weak, time::Instant};

pub(super) fn idle_cache(
    inner: &Weak<Inner>,
    index: usize,
    connection: &mut Option<ExecutionConnection>,
) -> bool {
    if super::cache_invalidation::service(inner, index, connection).is_err() {
        let cleanup = close_connection(inner, connection);
        super::cache_invalidation::closed(inner, index, cleanup.as_ref().err());
        if let Some(owner) = inner.upgrade() {
            let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stats.discarded += 1;
            state.stats.cleanup_errors += 1;
            if let Err(error) = cleanup {
                state.close_errors.push(error);
            }
        }
    }
    let Some(owner) = inner.upgrade() else {
        return true;
    };
    let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
    if state.closing {
        return true;
    }
    super::cache_invalidation::available(&mut state, index, connection.is_some());
    state.slots[index].idle_since = Instant::now();
    owner.dispatch(&mut state);
    owner.changed.notify_all();
    false
}

pub(super) fn close_connection(
    inner: &Weak<Inner>,
    connection: &mut Option<ExecutionConnection>,
) -> Result<(), Error> {
    if let Some(mut connection) = connection.take() {
        let counts = connection.take_cache_counts();
        if let Some(inner) = inner.upgrade() {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stats.statement_hits += counts.0;
            state.stats.statement_misses += counts.1;
        }
        connection.close()?;
    }
    Ok(())
}

pub(super) fn phase(inner: &Weak<Inner>, index: usize, phase: Phase) {
    if let Some(inner) = inner.upgrade() {
        inner.state.lock().unwrap_or_else(|e| e.into_inner()).slots[index].phase = phase;
    }
}

pub(super) fn retire_idle(
    inner: &Weak<Inner>,
    index: usize,
    config: &Config,
    last_used: Instant,
    born: Instant,
    lifetime: f64,
) -> bool {
    let Some(inner) = inner.upgrade() else {
        return true;
    };
    let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
    if !matches!(state.slots[index].phase, Phase::Idle | Phase::Dormant) {
        return false;
    }
    let idle_count = state
        .slots
        .iter()
        .filter(|s| s.phase == Phase::Idle)
        .count();
    let expired = lifetime > 0.0 && born.elapsed().as_secs_f64() >= lifetime;
    let idle = config.idle_timeout > 0.0
        && last_used.elapsed().as_secs_f64() >= config.idle_timeout
        && (state.slots[index].phase == Phase::Dormant || idle_count > config.min_idle as usize);
    if expired || idle || state.closing {
        state.slots[index].phase = Phase::Closing;
        true
    } else {
        false
    }
}

pub(super) fn acquire_timeout() -> Error {
    Error::new(
        ErrorKind::PoolTimeout,
        "acquire",
        "Pool acquisition deadline expired",
    )
}
