use crate::{errors::from_core, executor::Executor};
use fbfast_core::{
    error::{Error, ErrorKind},
    pool::{Output, Pending},
};
use pyo3::prelude::*;
use std::time::Duration;

pub(crate) fn wait(py: Python<'_>, pending: &mut Pending) -> PyResult<Output> {
    loop {
        if let Some(output) = py
            .detach(|| pending.wait_slice(Duration::from_millis(25)))
            .map_err(from_core)?
        {
            return Ok(output);
        }
        // KeyboardInterrupt drops the request, marks cancellation and leaves the worker counted.
        py.check_signals()?;
    }
}
pub(crate) fn close(py: Python<'_>, pool: &Executor, seconds: f64) -> PyResult<()> {
    let end = std::time::Instant::now()
        .checked_add(Duration::from_secs_f64(seconds))
        .ok_or_else(|| from_core(Error::config("close_timeout", "duration overflow")))?;
    loop {
        let remaining = end.saturating_duration_since(std::time::Instant::now());
        let result = py.detach(|| pool.close(remaining.min(Duration::from_millis(25))));
        match result {
            Err(error)
                if error.kind == ErrorKind::PoolTimeout && std::time::Instant::now() < end =>
            {
                py.check_signals()?;
            }
            result => return result.map_err(from_core),
        }
    }
}
pub(crate) fn internal() -> PyErr {
    from_core(Error::new(
        ErrorKind::Query,
        "delivery",
        "Unexpected native result type",
    ))
}
