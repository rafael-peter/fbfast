use crate::{config::PyConfig, errors::from_core, values::Converter};
use fbfast_core::{
    error::{Error, ErrorKind},
    pool::{Output, Pending, Pool},
};
use pyo3::{prelude::*, types::PyDict};
use std::{path::PathBuf, time::Duration};

#[pyclass(name = "_Pool", module = "fbfast._native", frozen)]
pub struct PyPool {
    inner: Pool,
    close_timeout: f64,
    delivery: std::sync::Arc<tokio::sync::Semaphore>,
}

#[pymethods]
impl PyPool {
    #[pyo3(signature = (sql, params=None, *, batch_rows=None, as_dict=false, timeout=None))]
    fn stream(
        &self,
        py: Python<'_>,
        sql: String,
        params: Option<&Bound<'_, PyAny>>,
        batch_rows: Option<u32>,
        as_dict: bool,
        timeout: Option<f64>,
    ) -> PyResult<crate::stream::PyStream> {
        self.inner.stats().map_err(from_core)?;
        let params = Converter::new(py)?.params(params)?;
        Ok(crate::stream::PyStream {
            core: self
                .inner
                .stream(sql, params, batch_rows, as_dict, timeout)
                .map_err(from_core)?,
            as_dict,
            close_timeout: self.close_timeout,
            delivery: self.delivery.clone(),
        })
    }
    #[new]
    fn new(config: &PyConfig, client_path: PathBuf) -> Self {
        Self {
            inner: Pool::new(config.inner.clone(), client_path),
            close_timeout: config.inner.close_timeout,
            delivery: std::sync::Arc::new(tokio::sync::Semaphore::new(
                config.inner.pool_size as usize + config.inner.max_waiters as usize,
            )),
        }
    }

    #[pyo3(signature = (sql, params=None, *, first=false, as_dict=false, timeout=None))]
    fn rows_async<'py>(
        &self,
        py: Python<'py>,
        sql: String,
        params: Option<&Bound<'py, PyAny>>,
        first: bool,
        as_dict: bool,
        timeout: Option<f64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        crate::asyncio::query(
            py,
            &self.inner,
            &self.delivery,
            sql,
            params,
            timeout,
            crate::asyncio::Form::Rows { first, as_dict },
        )
    }

    #[pyo3(signature = (sql, params=None, *, timeout=None))]
    fn value_async<'py>(
        &self,
        py: Python<'py>,
        sql: String,
        params: Option<&Bound<'py, PyAny>>,
        timeout: Option<f64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        crate::asyncio::query(
            py,
            &self.inner,
            &self.delivery,
            sql,
            params,
            timeout,
            crate::asyncio::Form::Value,
        )
    }

    #[pyo3(signature = (sql, params=None, *, timeout=None))]
    fn explain_async<'py>(
        &self,
        py: Python<'py>,
        sql: String,
        params: Option<&Bound<'py, PyAny>>,
        timeout: Option<f64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        crate::asyncio::query(
            py,
            &self.inner,
            &self.delivery,
            sql,
            params,
            timeout,
            crate::asyncio::Form::Plan,
        )
    }

    fn warmup<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::asyncio::warmup(py, &self.inner, &self.delivery, self.close_timeout)
    }

    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::asyncio::close_future(py, &self.inner, self.close_timeout)
    }

    #[pyo3(signature = (sql, params=None, *, first=false, as_dict=false, timeout=None))]
    fn rows(
        &self,
        py: Python<'_>,
        sql: String,
        params: Option<&Bound<'_, PyAny>>,
        first: bool,
        as_dict: bool,
        timeout: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        let mut converter = Converter::new(py)?;
        let params = converter.params(params)?;
        let mut pending = self
            .inner
            .submit_rows(sql, params, first, as_dict, timeout)
            .map_err(from_core)?;
        let output = wait(py, &mut pending)?;
        converter.set_budget(pending.delivery_budget().map_err(from_core)?);
        converter.checkpoint()?;
        let result = match output {
            Output::Rows(rows) => {
                if first {
                    converter.row(&rows, 0, as_dict)
                } else {
                    converter.all(&rows, as_dict)
                }
            }
            _ => Err(internal()),
        }?;
        converter.checkpoint()?;
        Ok(result)
    }

    #[pyo3(signature = (sql, params=None, *, timeout=None))]
    fn value(
        &self,
        py: Python<'_>,
        sql: String,
        params: Option<&Bound<'_, PyAny>>,
        timeout: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        let mut converter = Converter::new(py)?;
        let params = converter.params(params)?;
        let mut pending = self
            .inner
            .submit_rows(sql, params, true, false, timeout)
            .map_err(from_core)?;
        let output = wait(py, &mut pending)?;
        converter.set_budget(pending.delivery_budget().map_err(from_core)?);
        converter.checkpoint()?;
        let result = match output {
            Output::Rows(rows) => rows
                .row(0)
                .and_then(|row| row.first())
                .map_or_else(|| Ok(py.None()), |v| converter.value(v)),
            _ => Err(internal()),
        }?;
        converter.checkpoint()?;
        Ok(result)
    }

    #[pyo3(signature = (sql, params=None, *, timeout=None))]
    fn explain(
        &self,
        py: Python<'_>,
        sql: String,
        params: Option<&Bound<'_, PyAny>>,
        timeout: Option<f64>,
    ) -> PyResult<String> {
        let mut converter = Converter::new(py)?;
        let params = converter.params(params)?;
        let mut pending = self
            .inner
            .submit_explain(sql, params, timeout)
            .map_err(from_core)?;
        let output = wait(py, &mut pending)?;
        converter.set_budget(pending.delivery_budget().map_err(from_core)?);
        converter.checkpoint()?;
        match output {
            Output::Plan(plan) => Ok(plan),
            _ => Err(internal()),
        }
    }

    fn warmup_sync(&self, py: Python<'_>) -> PyResult<()> {
        let mut requests = self.inner.warmup().map_err(from_core)?;
        for request in &mut requests {
            if let Err(error) = wait(py, request) {
                // Roll back partial initialization; close does not hide the opening error.
                if let Err(cleanup) = close(py, &self.inner, self.close_timeout) {
                    error.set_cause(py, Some(cleanup));
                }
                return Err(error);
            }
        }
        Ok(())
    }

    fn close_sync(&self, py: Python<'_>) -> PyResult<()> {
        close(py, &self.inner, self.close_timeout)
    }

    #[pyo3(signature = (*, timeout=None))]
    fn clear_statement_cache_sync(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<()> {
        let mut pending = self
            .inner
            .clear_statement_cache(timeout)
            .map_err(from_core)?;
        match wait(py, &mut pending)? {
            Output::Ready => pending
                .delivery_budget()
                .and_then(|budget| budget.check_phase("cache_clear"))
                .map_err(from_core),
            _ => Err(internal()),
        }
    }

    #[pyo3(signature = (*, timeout=None))]
    fn clear_statement_cache<'py>(
        &self,
        py: Python<'py>,
        timeout: Option<f64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        crate::asyncio::clear_cache(py, &self.inner, &self.delivery, timeout)
    }

    fn stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let values =
            serde_json::to_value(self.inner.stats().map_err(from_core)?).map_err(|_| internal())?;
        let result = PyDict::new(py);
        for (name, value) in values.as_object().ok_or_else(internal)? {
            if let Some(value) = value.as_bool() {
                result.set_item(name, value)?;
            } else {
                result.set_item(name, value.as_u64().ok_or_else(internal)?)?;
            }
        }
        Ok(result)
    }
}

fn wait(py: Python<'_>, pending: &mut Pending) -> PyResult<Output> {
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
fn close(py: Python<'_>, pool: &Pool, seconds: f64) -> PyResult<()> {
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
fn internal() -> PyErr {
    from_core(Error::new(
        ErrorKind::Query,
        "delivery",
        "Unexpected native result type",
    ))
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<crate::stream::PyStream>()?;
    module.add_class::<PyPool>()
}
