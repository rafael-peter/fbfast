//! Await the same bounded native scheduler; materialize off the Python loop.
use crate::{
    async_runtime::{self, CancelOnDrop, Control},
    errors::from_core,
    values::Converter,
};
use fbfast_core::{
    error::{Error, ErrorKind},
    pool::{Output, Pending, Pool},
};
use pyo3::{exceptions::PyRuntimeError, prelude::*, types::PyList, IntoPyObjectExt};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy)]
pub enum Form {
    Rows { first: bool, as_dict: bool },
    Value,
    Plan,
}

fn admission(gate: &Arc<Semaphore>) -> PyResult<Arc<OwnedSemaphorePermit>> {
    gate.clone().try_acquire_owned().map(Arc::new).map_err(|_| {
        from_core(Error::new(
            ErrorKind::PoolBusy,
            "delivery",
            "Async result delivery capacity is full",
        ))
    })
}

pub fn query<'py>(
    py: Python<'py>,
    pool: &Pool,
    gate: &Arc<Semaphore>,
    sql: String,
    params: Option<&Bound<'py, PyAny>>,
    timeout: Option<f64>,
    form: Form,
) -> PyResult<Bound<'py, PyAny>> {
    // Check fork before the runtime singleton or admission semaphore is touched.
    pool.stats().map_err(from_core)?;
    let (locals, control) = async_runtime::context(py)?;
    let permit = admission(gate)?;
    let parameters = Converter::new(py)?.params(params)?;
    let pending = match form {
        Form::Plan => pool.submit_explain(sql, parameters, timeout),
        Form::Value => pool.submit_rows(sql, parameters, true, false, timeout),
        Form::Rows { first, as_dict } => pool.submit_rows(sql, parameters, first, as_dict, timeout),
    }
    .map_err(from_core)?;
    pyo3_async_runtimes::tokio::future_into_py_with_locals(py, locals, async move {
        let _cancel = CancelOnDrop(control.clone());
        let mut pending = pending;
        let output = wait(&mut pending, &control).await?;
        let budget = pending.delivery_budget().map_err(from_core)?;
        tokio::task::spawn_blocking(move || {
            // Keep admission counted until a cancelled materializer really exits.
            let _permit = permit;
            Python::attach(|py| {
                if let Err(error) = control.check(py) {
                    budget.cancel();
                    return Err(error);
                }
                let mut converter = Converter::new(py)?;
                converter.set_budget(budget);
                converter.set_async_control(control);
                converter.checkpoint()?;
                // Borrow buffers so their potentially large deallocation happens
                // after Python::attach returns and the GIL has been released.
                let result = match (&output, form) {
                    (Output::Rows(rows), Form::Rows { first, as_dict }) => {
                        if first {
                            converter.row(rows, 0, as_dict)
                        } else {
                            let result = PyList::empty(py);
                            for index in 0..rows.count {
                                result.append(converter.row(rows, index, as_dict)?)?;
                            }
                            Ok(result.into_any().unbind())
                        }
                    }
                    (Output::Rows(rows), Form::Value) => rows
                        .row(0)
                        .and_then(|row| row.first())
                        .map_or_else(|| Ok(py.None()), |value| converter.value(value)),
                    (Output::Plan(plan), Form::Plan) => plan.into_py_any(py),
                    _ => Err(PyRuntimeError::new_err("Unexpected native async output")),
                }?;
                converter.checkpoint()?;
                Ok(result)
            })
        })
        .await
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
    })
}

async fn wait(pending: &mut Pending, control: &Arc<Control>) -> PyResult<Output> {
    loop {
        match tokio::time::timeout(Duration::from_millis(25), pending.receive()).await {
            Ok(result) => return result.map_err(from_core),
            Err(_) => {
                control.check_async().await?;
                if let Some(result) = pending.wait_slice(Duration::ZERO).map_err(from_core)? {
                    return Ok(result);
                }
            }
        }
    }
}

pub fn clear_cache<'py>(
    py: Python<'py>,
    pool: &Pool,
    gate: &Arc<Semaphore>,
    timeout: Option<f64>,
) -> PyResult<Bound<'py, PyAny>> {
    pool.stats().map_err(from_core)?;
    let (locals, control) = async_runtime::context(py)?;
    let permit = admission(gate)?;
    let mut pending = pool.clear_statement_cache(timeout).map_err(from_core)?;
    pyo3_async_runtimes::tokio::future_into_py_with_locals(py, locals, async move {
        let _permit = permit;
        let _cancel = CancelOnDrop(control.clone());
        match wait(&mut pending, &control).await? {
            Output::Ready => Ok(()),
            _ => Err(PyRuntimeError::new_err(
                "Unexpected cache invalidation output",
            )),
        }
    })
}

pub fn warmup<'py>(
    py: Python<'py>,
    pool: &Pool,
    gate: &Arc<Semaphore>,
    close_timeout: f64,
) -> PyResult<Bound<'py, PyAny>> {
    pool.stats().map_err(from_core)?;
    let (locals, control) = async_runtime::context(py)?;
    let permit = admission(gate)?;
    let mut requests = pool.warmup().map_err(from_core)?;
    let pool = pool.clone();
    pyo3_async_runtimes::tokio::future_into_py_with_locals(py, locals, async move {
        let _permit = permit;
        let _cancel = CancelOnDrop(control.clone());
        let mut rollback = WarmupRollback {
            pool: pool.clone(),
            complete: false,
        };
        for request in &mut requests {
            if let Err(error) = wait(request, &control).await {
                drop(requests);
                if let Err(cleanup) = close(&pool, close_timeout, &control).await {
                    let error = tokio::task::spawn_blocking(move || {
                        Python::attach(|py| error.set_cause(py, Some(cleanup)));
                        error
                    })
                    .await
                    .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
                    return Err(error);
                }
                return Err(error);
            }
        }
        rollback.complete = true;
        Ok(())
    })
}

struct WarmupRollback {
    pool: Pool,
    complete: bool,
}
impl Drop for WarmupRollback {
    fn drop(&mut self) {
        if !self.complete {
            let _ = self.pool.begin_close();
        }
    }
}

pub fn close_future<'py>(
    py: Python<'py>,
    pool: &Pool,
    seconds: f64,
) -> PyResult<Bound<'py, PyAny>> {
    pool.stats().map_err(from_core)?;
    let (locals, control) = async_runtime::context(py)?;
    pool.begin_close().map_err(from_core)?;
    let pool = pool.clone();
    pyo3_async_runtimes::tokio::future_into_py_with_locals(py, locals, async move {
        let _cancel = CancelOnDrop(control.clone());
        close(&pool, seconds, &control).await
    })
}

async fn close(pool: &Pool, seconds: f64, control: &Arc<Control>) -> PyResult<()> {
    let end = Instant::now()
        .checked_add(Duration::from_secs_f64(seconds))
        .ok_or_else(|| from_core(Error::config("close_timeout", "duration overflow")))?;
    loop {
        match pool.close(Duration::ZERO) {
            Err(error) if error.kind == ErrorKind::PoolTimeout && Instant::now() < end => {
                control.check_async().await?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            result => return result.map_err(from_core),
        }
    }
}
