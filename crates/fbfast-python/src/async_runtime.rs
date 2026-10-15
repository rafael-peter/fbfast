//! Process-bounded Tokio coordination and per-call Python loop ownership.
use crate::errors::from_core;
use fbfast_core::error::{Error, ErrorKind};
use pyo3::{
    exceptions::PyRuntimeError,
    prelude::*,
    types::{PyDict, PyTuple},
};
use pyo3_async_runtimes::TaskLocals;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, OnceLock,
};

static OWNER: AtomicU32 = AtomicU32::new(0);
static INITIALIZED: OnceLock<Result<(), String>> = OnceLock::new();

pub fn initialize() -> PyResult<()> {
    let pid = std::process::id();
    let owner = OWNER
        .compare_exchange(0, pid, Ordering::SeqCst, Ordering::SeqCst)
        .unwrap_or_else(|owner| owner);
    if owner != 0 && owner != pid {
        return Err(from_core(Error::new(
            ErrorKind::ForkSafety,
            "async_runtime",
            "Async runtime belongs to another process; use spawn",
        )));
    }
    INITIALIZED
        .get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .max_blocking_threads(2)
                .enable_time()
                .thread_name("fbfast-async")
                .build()
                .map_err(|e| e.to_string())?;
            // The bridge's runtime has process lifetime. No thread is created by import.
            pyo3_async_runtimes::tokio::init_with_runtime(Box::leak(Box::new(runtime)))
                .map_err(|()| "Async bridge was initialized unexpectedly".to_owned())
        })
        .as_ref()
        .map_err(|error| PyRuntimeError::new_err(error.clone()))?;
    Ok(())
}

pub struct Control {
    event_loop: Py<PyAny>,
    pub cancelled: AtomicBool,
}

impl Control {
    pub fn check(&self, py: Python<'_>) -> PyResult<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(from_core(Error::new(
                ErrorKind::Cancelled,
                "delivery",
                "Async delivery cancelled",
            )));
        }
        if self
            .event_loop
            .bind(py)
            .call_method0("is_closed")?
            .extract::<bool>()?
        {
            self.cancelled.store(true, Ordering::Release);
            return Err(PyRuntimeError::new_err("Originating event loop is closed"));
        }
        Ok(())
    }

    pub async fn check_async(self: &Arc<Self>) -> PyResult<()> {
        let control = self.clone();
        tokio::task::spawn_blocking(move || Python::attach(|py| control.check(py)))
            .await
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
    }
}

pub struct CancelOnDrop(pub Arc<Control>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancelled.store(true, Ordering::Release);
    }
}

/// The upstream bridge schedules completion even if the originating loop closed.
/// This per-call proxy gates that one scheduling method; it never patches a loop
/// or stores a global Python loop, and preserves the caller's contextvars.
#[pyclass(frozen)]
struct LoopGate {
    control: Arc<Control>,
}

#[pymethods]
impl LoopGate {
    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        Ok(self.control.event_loop.bind(py).getattr(name)?.unbind())
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn call_soon_threadsafe(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let event_loop = self.control.event_loop.bind(py);
        if event_loop.call_method0("is_closed")?.extract::<bool>()? {
            return Ok(py.None());
        }
        match event_loop.call_method("call_soon_threadsafe", args, kwargs) {
            Ok(value) => Ok(value.unbind()),
            Err(error)
                if error.is_instance_of::<PyRuntimeError>(py)
                    && event_loop.call_method0("is_closed")?.extract::<bool>()? =>
            {
                Ok(py.None())
            }
            Err(error) => Err(error),
        }
    }
}

pub fn context(py: Python<'_>) -> PyResult<(TaskLocals, Arc<Control>)> {
    initialize()?;
    let original = pyo3_async_runtimes::tokio::get_current_locals(py)?;
    let control = Arc::new(Control {
        event_loop: original.event_loop(py).unbind(),
        cancelled: AtomicBool::new(false),
    });
    let gate = Bound::new(
        py,
        LoopGate {
            control: control.clone(),
        },
    )?;
    Ok((
        TaskLocals::new(gate.into_any()).with_context(original.context(py)),
        control,
    ))
}
