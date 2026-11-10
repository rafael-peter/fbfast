//! Native errors become instances of the Python hierarchy in `fbfast._errors`.
//! Classes are defined in Python so they can also derive from built-in
//! exceptions such as `TimeoutError` and `ConnectionError`.
use fbfast_core::error::{Error, ErrorKind};
use pyo3::{exceptions::PyRuntimeError, prelude::*, sync::PyOnceLock, types::PyDict};

static MODULE: PyOnceLock<Py<PyModule>> = PyOnceLock::new();

fn class_name(error: &Error) -> &'static str {
    match error.kind {
        ErrorKind::Configuration => "ConfigurationError",
        ErrorKind::UnsupportedFeature => "UnsupportedFeatureError",
        ErrorKind::UnsupportedStatement => "UnsupportedStatementError",
        ErrorKind::ReadOnlyViolation => "ReadOnlyViolationError",
        ErrorKind::ClientLibrary => "ClientLibraryError",
        ErrorKind::ForkSafety => "ForkSafetyError",
        ErrorKind::Query if is_connection_failure(error) => "DatabaseConnectionError",
        ErrorKind::Query => "QueryError",
        ErrorKind::QueryTimeout => "QueryTimeoutError",
        ErrorKind::Authentication => "AuthenticationError",
        ErrorKind::RoleNotActive => "RoleNotActiveError",
        ErrorKind::UnsupportedServer => "UnsupportedServerError",
        ErrorKind::DataConversion => "DataConversionError",
        ErrorKind::UnsupportedType => "UnsupportedTypeError",
        ErrorKind::ResultLimit => "ResultLimitError",
        ErrorKind::Cleanup => "CleanupError",
        ErrorKind::PoolClosed => "PoolClosedError",
        ErrorKind::PoolBusy => "PoolBusyError",
        ErrorKind::ConcurrentUse => "ConcurrentUseError",
        ErrorKind::PoolTimeout => "PoolTimeoutError",
        ErrorKind::Cancelled => "QueryCancelledError",
    }
}

/// Firebird reported a connection exception (SQLSTATE class 08), or the server
/// or network refused the attach itself. Handshake policy mismatches detected
/// by fbfast (charset, time zone, encryption policy) carry no SQLSTATE and stay
/// plain QueryError: retrying cannot fix them.
fn is_connection_failure(error: &Error) -> bool {
    let state = error.sqlstate.as_deref();
    state.is_some_and(|state| state.starts_with("08"))
        || (error.phase == "attach" && state.is_some_and(|state| state != "28000"))
}

fn instance(py: Python<'_>, error: Error) -> PyResult<Bound<'_, PyAny>> {
    let module = MODULE.get_or_try_init(py, || py.import("fbfast._errors").map(Bound::unbind))?;
    let class = module.bind(py).getattr(class_name(&error))?;
    let cleanup = error
        .cleanup
        .into_iter()
        .map(|child| instance(py, child))
        .collect::<PyResult<Vec<_>>>()?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("phase", error.phase)?;
    kwargs.set_item("sqlstate", error.sqlstate)?;
    kwargs.set_item("native_codes", error.native_codes)?;
    kwargs.set_item("cleanup_errors", cleanup)?;
    class.call((error.message,), Some(&kwargs))
}

/// Every native failure reaches Python through this function, so public errors
/// always carry `phase`, `sqlstate`, `native_codes` and `cleanup_errors`.
pub fn from_core(error: Error) -> PyErr {
    Python::attach(|py| {
        let message = error.message.clone();
        match instance(py, error) {
            Ok(value) => PyErr::from_value(value),
            Err(failure) => {
                let fallback = PyRuntimeError::new_err(message);
                fallback.set_cause(py, Some(failure));
                fallback
            }
        }
    })
}

/// Configuration failures raised directly by the binding (keyword parsing).
pub fn configuration(message: impl Into<String>) -> PyErr {
    from_core(Error::new(ErrorKind::Configuration, "config", message))
}
