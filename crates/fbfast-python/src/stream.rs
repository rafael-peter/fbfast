use crate::{
    async_runtime::{self, CancelOnDrop, Control},
    errors::from_core,
    values::Converter,
};
use fbfast_core::{
    error::{Error, ErrorKind},
    pool::{Stream, StreamAction, StreamItem, StreamWait},
};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

#[pyclass(name = "_Stream", module = "fbfast._native", frozen)]
pub struct PyStream {
    pub core: Arc<Stream>,
    pub exported: Mutex<bool>,
    pub as_dict: bool,
    pub close_timeout: f64,
    pub delivery: Arc<Semaphore>,
}

struct Delivery {
    wait: StreamWait,
    delivered: bool,
}
impl Drop for Delivery {
    fn drop(&mut self) {
        if !self.delivered {
            self.wait.cancel();
        }
    }
}

#[pymethods]
impl PyStream {
    #[pyo3(signature = (requested_schema=None))]
    fn __arrow_c_stream__<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, pyo3::types::PyCapsule>> {
        crate::arrow_stream::export(self, py, requested_schema)
    }
    #[getter]
    fn schema(&self) -> PyResult<crate::arrow_batch::Schema> {
        Ok(crate::arrow_batch::Schema {
            schema: self.core.arrow_schema().map_err(from_core)?,
        })
    }
    fn open_sync(&self, py: Python<'_>) -> PyResult<()> {
        self.sync(py, StreamAction::Open).map(|_| ())
    }
    fn next_sync(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.sync(py, StreamAction::Next)
    }
    fn close_sync(&self, py: Python<'_>) -> PyResult<()> {
        self.sync(py, StreamAction::Close).map(|_| ())
    }
    fn open_async<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.asynchronous(py, StreamAction::Open)
    }
    fn next_async<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.asynchronous(py, StreamAction::Next)
    }
    fn close_async<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.asynchronous(py, StreamAction::Close)
    }
}

impl PyStream {
    pub fn new(core: Stream, as_dict: bool, close_timeout: f64, delivery: Arc<Semaphore>) -> Self {
        Self {
            core: Arc::new(core),
            exported: Mutex::new(false),
            as_dict,
            close_timeout,
            delivery,
        }
    }
    fn request(&self, action: StreamAction) -> Result<StreamWait, Error> {
        self.core.check_pid()?;
        let exported = self.exported.lock().unwrap_or_else(|e| e.into_inner());
        if *exported && action != StreamAction::Close {
            return Err(crate::arrow_stream::busy());
        }
        if *exported {
            self.core.stop(false)?;
        }
        self.core.request(action)
    }
    fn sync(&self, py: Python<'_>, action: StreamAction) -> PyResult<Option<Py<PyAny>>> {
        let end = close_end(action, self.close_timeout);
        let (wait, cancelled_on_close) = self.request_sync(py, action, end)?;
        let mut delivery = Delivery {
            wait,
            delivered: false,
        };
        let item = loop {
            check_close(end)?;
            let result = py.detach(|| {
                let result = delivery.wait.poll();
                if matches!(result, Ok(None)) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                result
            });
            // Explicit context close interrupted the foreign reader. That reader
            // receives cancellation; close only acknowledges completed cleanup.
            let result = match result {
                Err(e) if cancelled_on_close && e.kind == ErrorKind::Cancelled => {
                    delivery.delivered = true;
                    return Ok(None);
                }
                result => result.map_err(from_core)?,
            };
            py.check_signals()?;
            if let Some(item) = result {
                break item;
            }
        };
        let result = match &item {
            StreamItem::Arrow(batch) => {
                delivery
                    .wait
                    .budget()
                    .map_err(from_core)?
                    .check()
                    .map_err(from_core)?;
                Some(crate::arrow_batch::convert(py, batch)?)
            }
            StreamItem::Json(json) => {
                let mut converter = Converter::new(py)?;
                converter.set_budget(delivery.wait.budget().map_err(from_core)?);
                Some(converter.bytes(&json.data)?)
            }
            StreamItem::Batch(rows) => {
                let mut converter = Converter::new(py)?;
                converter.set_budget(delivery.wait.budget().map_err(from_core)?);
                let result = converter.all(rows, self.as_dict)?;
                converter.checkpoint()?;
                Some(result)
            }
            _ => None,
        };
        // Large native buffers are released without holding the interpreter.
        py.detach(|| drop(item));
        delivery.delivered = true;
        Ok(result)
    }

    fn request_sync(
        &self,
        py: Python<'_>,
        action: StreamAction,
        end: Option<Instant>,
    ) -> PyResult<(StreamWait, bool)> {
        let mut cancelled = false;
        loop {
            check_close(end)?;
            match self.request(action) {
                Ok(wait) => return Ok((wait, cancelled)),
                Err(e)
                    if action == StreamAction::Close
                        && e.kind == ErrorKind::PoolBusy
                        && *self.exported.lock().unwrap_or_else(|e| e.into_inner()) =>
                {
                    self.core.stop(true).map_err(from_core)?;
                    cancelled = true;
                    py.detach(|| std::thread::sleep(Duration::from_millis(5)));
                    py.check_signals()?;
                }
                Err(e) => return Err(from_core(e)),
            }
        }
    }

    fn asynchronous<'py>(
        &self,
        py: Python<'py>,
        action: StreamAction,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Request verifies PID before runtime/semaphore access and reserves the
        // exclusive consumer through result materialization, not just fetch.
        let mut delivery = Delivery {
            wait: self.request(action).map_err(from_core)?,
            delivered: false,
        };
        let (locals, control) = async_runtime::context(py)?;
        let permit = if action == StreamAction::Close {
            None
        } else {
            Some(crate::asyncio::admission(&self.delivery)?)
        };
        let end = close_end(action, self.close_timeout);
        let as_dict = self.as_dict;
        pyo3_async_runtimes::tokio::future_into_py_with_locals(py, locals, async move {
            let _cancel = CancelOnDrop(control.clone());
            let item = wait(&mut delivery.wait, &control, end).await?;
            if matches!(
                &item,
                StreamItem::Batch(_) | StreamItem::Json(_) | StreamItem::Arrow(_)
            ) {
                tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    let result = Python::attach(|py| {
                        control.check(py)?;
                        let mut converter = Converter::new(py)?;
                        converter.set_budget(delivery.wait.budget().map_err(from_core)?);
                        converter.set_async_control(control);
                        let result = match &item {
                            StreamItem::Arrow(batch) => crate::arrow_batch::convert(py, batch)?,
                            StreamItem::Batch(rows) => converter.all(rows, as_dict)?,
                            StreamItem::Json(json) => converter.bytes(&json.data)?,
                            _ => unreachable!("batch checked before conversion"),
                        };
                        converter.checkpoint()?;
                        Ok(Some(result))
                    });
                    if result.is_ok() {
                        delivery.delivered = true;
                    }
                    result
                })
                .await
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
            } else {
                control.check_async().await?;
                delivery.delivered = true;
                Ok(None)
            }
        })
    }
}

async fn wait(
    wait: &mut StreamWait,
    control: &Arc<Control>,
    end: Option<Instant>,
) -> PyResult<StreamItem> {
    loop {
        check_close(end)?;
        match tokio::time::timeout(Duration::from_millis(25), wait.receive()).await {
            Ok(result) => {
                if let Some(item) = result.map_err(from_core)? {
                    return Ok(item);
                }
            }
            Err(_) => {
                control.check_async().await?;
            }
        }
    }
}
fn close_end(action: StreamAction, seconds: f64) -> Option<Instant> {
    (action == StreamAction::Close).then(|| Instant::now() + Duration::from_secs_f64(seconds))
}
fn check_close(end: Option<Instant>) -> PyResult<()> {
    if end.is_some_and(|end| Instant::now() >= end) {
        Err(from_core(Error::new(
            ErrorKind::PoolTimeout,
            "stream_close",
            "Stream cleanup deadline expired; native slot remains counted",
        )))
    } else {
        Ok(())
    }
}

pub enum Format {
    Rows(bool),
    Arrow,
}

pub fn create(
    py: Python<'_>,
    pool: &crate::pool::PyPool,
    sql: String,
    params: Option<&Bound<'_, PyAny>>,
    batch_rows: Option<u32>,
    timeout: Option<f64>,
    format: Format,
) -> PyResult<PyStream> {
    pool.inner.stats().map_err(from_core)?;
    let params = Converter::new(py)?.params(params)?;
    let as_dict = matches!(format, Format::Rows(true));
    let core = match format {
        Format::Arrow => pool.inner.stream_arrow(sql, params, batch_rows, timeout),
        Format::Rows(_) => pool.inner.stream(sql, params, batch_rows, as_dict, timeout),
    }
    .map_err(from_core)?;
    Ok(crate::stream::PyStream::new(
        core,
        as_dict,
        pool.close_timeout,
        pool.delivery.clone(),
    ))
}
