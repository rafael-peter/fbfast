use fbfast_core::error::{Error, ErrorKind};
use pyo3::{create_exception, exceptions::PyException, prelude::*};

create_exception!(fbfast, FbFastError, PyException);
create_exception!(fbfast, ConfigurationError, FbFastError);
create_exception!(fbfast, UnsupportedFeatureError, FbFastError);
create_exception!(fbfast, UnsupportedStatementError, FbFastError);
create_exception!(fbfast, ReadOnlyViolationError, FbFastError);
create_exception!(fbfast, ClientLibraryError, FbFastError);
create_exception!(fbfast, PoolError, FbFastError);
create_exception!(fbfast, ForkSafetyError, PoolError);
create_exception!(fbfast, PoolClosedError, PoolError);
create_exception!(fbfast, PoolBusyError, PoolError);
create_exception!(fbfast, PoolTimeoutError, PoolError);
create_exception!(fbfast, QueryCancelledError, FbFastError);
create_exception!(fbfast, QueryError, FbFastError);
create_exception!(fbfast, QueryTimeoutError, QueryError);
create_exception!(fbfast, AuthenticationError, FbFastError);
create_exception!(fbfast, RoleNotActiveError, FbFastError);
create_exception!(fbfast, UnsupportedServerError, FbFastError);
create_exception!(fbfast, DataConversionError, FbFastError);
create_exception!(fbfast, UnsupportedTypeError, FbFastError);
create_exception!(fbfast, ResultLimitError, FbFastError);
create_exception!(fbfast, CleanupError, FbFastError);

pub fn from_core(error: Error) -> PyErr {
    let result = match error.kind {
        ErrorKind::Configuration => ConfigurationError::new_err(error.message),
        ErrorKind::UnsupportedFeature => UnsupportedFeatureError::new_err(error.message),
        ErrorKind::UnsupportedStatement => UnsupportedStatementError::new_err(error.message),
        ErrorKind::ReadOnlyViolation => ReadOnlyViolationError::new_err(error.message),
        ErrorKind::ClientLibrary => ClientLibraryError::new_err(error.message),
        ErrorKind::ForkSafety => ForkSafetyError::new_err(error.message),
        ErrorKind::Query => QueryError::new_err(error.message),
        ErrorKind::QueryTimeout => QueryTimeoutError::new_err(error.message),
        ErrorKind::Authentication => AuthenticationError::new_err(error.message),
        ErrorKind::RoleNotActive => RoleNotActiveError::new_err(error.message),
        ErrorKind::UnsupportedServer => UnsupportedServerError::new_err(error.message),
        ErrorKind::DataConversion => DataConversionError::new_err(error.message),
        ErrorKind::UnsupportedType => UnsupportedTypeError::new_err(error.message),
        ErrorKind::ResultLimit => ResultLimitError::new_err(error.message),
        ErrorKind::Cleanup => CleanupError::new_err(error.message),
        ErrorKind::PoolClosed => PoolClosedError::new_err(error.message),
        ErrorKind::PoolBusy => PoolBusyError::new_err(error.message),
        ErrorKind::PoolTimeout => PoolTimeoutError::new_err(error.message),
        ErrorKind::Cancelled => QueryCancelledError::new_err(error.message),
    };
    Python::attach(|py| {
        // Public errors carry the failure phase even when there is no server status.
        let _ = result.value(py).setattr("phase", error.phase);
        let _ = result.value(py).setattr("sqlstate", error.sqlstate);
        let _ = result.value(py).setattr("native_codes", error.native_codes);
        let children: Vec<_> = error
            .cleanup
            .into_iter()
            .map(|e| from_core(e).into_value(py))
            .collect();
        let _ = result.value(py).setattr("cleanup_errors", children);
    });
    result
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    module.add("PoolClosedError", py.get_type::<PoolClosedError>())?;
    module.add("PoolBusyError", py.get_type::<PoolBusyError>())?;
    module.add("PoolTimeoutError", py.get_type::<PoolTimeoutError>())?;
    module.add("QueryCancelledError", py.get_type::<QueryCancelledError>())?;
    module.add("QueryError", py.get_type::<QueryError>())?;
    module.add("QueryTimeoutError", py.get_type::<QueryTimeoutError>())?;
    module.add("AuthenticationError", py.get_type::<AuthenticationError>())?;
    module.add("RoleNotActiveError", py.get_type::<RoleNotActiveError>())?;
    module.add(
        "UnsupportedServerError",
        py.get_type::<UnsupportedServerError>(),
    )?;
    module.add("DataConversionError", py.get_type::<DataConversionError>())?;
    module.add(
        "UnsupportedTypeError",
        py.get_type::<UnsupportedTypeError>(),
    )?;
    module.add("ResultLimitError", py.get_type::<ResultLimitError>())?;
    module.add("CleanupError", py.get_type::<CleanupError>())?;
    module.add("ClientLibraryError", py.get_type::<ClientLibraryError>())?;
    module.add("PoolError", py.get_type::<PoolError>())?;
    module.add("ForkSafetyError", py.get_type::<ForkSafetyError>())?;
    module.add("FbFastError", py.get_type::<FbFastError>())?;
    module.add("ConfigurationError", py.get_type::<ConfigurationError>())?;
    module.add(
        "UnsupportedFeatureError",
        py.get_type::<UnsupportedFeatureError>(),
    )?;
    module.add(
        "UnsupportedStatementError",
        py.get_type::<UnsupportedStatementError>(),
    )?;
    module.add(
        "ReadOnlyViolationError",
        py.get_type::<ReadOnlyViolationError>(),
    )?;
    Ok(())
}
