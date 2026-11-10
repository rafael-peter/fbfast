use crate::executor::Executor;
use crate::pool_wait::{close, internal, wait};
use crate::{config::PyConfig, delivery::Form, errors::from_core, values::Converter};
use fbfast_core::pool::{Output, Pool, StreamFormat, StreamQuery};
use pyo3::{
    prelude::*,
    types::{PyDict, PyList},
};
use std::path::PathBuf;

#[pyclass(name = "_Pool", module = "fbfast._native", frozen)]
pub struct PyPool {
    pub(crate) inner: Executor,
    pub(crate) close_timeout: f64,
    pub(crate) delivery: std::sync::Arc<tokio::sync::Semaphore>,
}

#[pymethods]
impl PyPool {
    #[pyo3(signature = (sql, params=None, *, mode="arrow", timeout=None, asynchronous=false))]
    fn arrow<'py>(
        &self,
        py: Python<'py>,
        sql: String,
        params: Option<&Bound<'py, PyAny>>,
        mode: &str,
        timeout: Option<f64>,
        asynchronous: bool,
    ) -> PyResult<Py<PyAny>> {
        self.inner.stats().map_err(from_core)?;
        crate::arrow::query(
            py,
            self,
            sql,
            params,
            crate::arrow::mode(mode)?,
            timeout,
            asynchronous,
        )
    }
    #[pyo3(signature = (sql, params=None, *, options, timeout=None, asynchronous=false))]
    fn json<'py>(
        &self,
        py: Python<'py>,
        sql: String,
        params: Option<&Bound<'py, PyAny>>,
        options: Vec<String>,
        timeout: Option<f64>,
        asynchronous: bool,
    ) -> PyResult<Py<PyAny>> {
        self.inner.stats().map_err(from_core)?;
        crate::json::query(
            py,
            self,
            sql,
            params,
            crate::json::options(&options)?,
            timeout,
            asynchronous,
        )
    }
    #[pyo3(signature = (sql, params=None, *, options, batch_rows=None, timeout=None))]
    fn stream_json(
        &self,
        py: Python<'_>,
        sql: String,
        params: Option<&Bound<'_, PyAny>>,
        options: (Vec<String>, String),
        batch_rows: Option<u32>,
        timeout: Option<f64>,
    ) -> PyResult<crate::stream::PyStream> {
        self.inner.stats().map_err(from_core)?;
        let json = (
            crate::json::options(&options.0)?,
            crate::json::format(&options.1)?,
        );
        let params = Converter::new(py)?.params(params)?;
        let core = self
            .inner
            .open_stream(StreamQuery {
                sql,
                params,
                timeout,
                batch_rows,
                format: StreamFormat::Json(json.0, json.1),
            })
            .map_err(from_core)?;
        Ok(crate::stream::PyStream::new(
            core,
            false,
            self.close_timeout,
            self.delivery.clone(),
        ))
    }
    #[pyo3(signature = (*, timeout=None))]
    fn snapshot(&self, timeout: Option<f64>) -> PyResult<Self> {
        Ok(Self {
            inner: self.inner.snapshot(timeout).map_err(from_core)?,
            close_timeout: self.close_timeout,
            delivery: self.delivery.clone(),
        })
    }
    #[pyo3(signature = (sql, params=None, *, batch_rows=None, as_dict=false, timeout=None, arrow=false))]
    // Keep the existing Python row-stream keywords while selecting Arrow privately.
    #[allow(clippy::too_many_arguments)]
    fn stream(
        &self,
        py: Python<'_>,
        sql: String,
        params: Option<&Bound<'_, PyAny>>,
        batch_rows: Option<u32>,
        as_dict: bool,
        timeout: Option<f64>,
        arrow: bool,
    ) -> PyResult<crate::stream::PyStream> {
        let format = if arrow {
            crate::stream::Format::Arrow
        } else {
            crate::stream::Format::Rows(as_dict)
        };
        crate::stream::create(py, self, sql, params, batch_rows, timeout, format)
    }
    #[new]
    fn new(config: &PyConfig, client_path: PathBuf) -> Self {
        Self {
            inner: Executor::Pool(Pool::new(config.inner.clone(), client_path)),
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
        let form = Form::Rows { first, as_dict };
        crate::delivery::run(py, &self.inner, sql, params, timeout, form)
    }

    #[pyo3(signature = (sql, params=None, *, timeout=None))]
    fn value(
        &self,
        py: Python<'_>,
        sql: String,
        params: Option<&Bound<'_, PyAny>>,
        timeout: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        crate::delivery::run(py, &self.inner, sql, params, timeout, Form::Value)
    }

    #[pyo3(signature = (sql, params=None, *, timeout=None))]
    fn explain(
        &self,
        py: Python<'_>,
        sql: String,
        params: Option<&Bound<'_, PyAny>>,
        timeout: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        crate::delivery::run(py, &self.inner, sql, params, timeout, Form::Plan)
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

    fn request_close(&self) -> PyResult<()> {
        self.inner.begin_close().map_err(from_core)
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

    /// Runs independent queries with at most `concurrency` in flight.
    #[pyo3(signature = (specs, *, concurrency, timeout=None))]
    fn group(
        &self,
        py: Python<'_>,
        specs: Vec<crate::group::Spec<'_>>,
        concurrency: usize,
        timeout: Option<f64>,
    ) -> PyResult<crate::group::PyGroup> {
        crate::group::PyGroup::new(py, &self.inner, specs, concurrency, timeout)
    }

    fn session_info(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        crate::introspection::session_info(py, self.inner.session_info().map_err(from_core)?)
    }

    fn diagnostics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        crate::introspection::diagnostics(py, self.inner.diagnostics().map_err(from_core)?)
    }

    fn stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        crate::introspection::stats(py, self.inner.stats().map_err(from_core)?)
    }
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<crate::group::PyGroup>()?;
    module.add_class::<crate::stream::PyStream>()?;
    module.add_class::<PyPool>()
}
