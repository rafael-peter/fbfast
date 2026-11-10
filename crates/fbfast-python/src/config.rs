use crate::errors::{configuration, from_core};
use fbfast_core::config::{Config, EnvFile, Inputs};
use pyo3::{
    exceptions::PyAttributeError,
    prelude::*,
    types::{PyBool, PyDict, PyType},
    IntoPyObjectExt,
};
use serde_json::{Number, Value};

/// Immutable connection and pool settings; FirebirdConfig() reads FB_* and .env.
///
/// Precedence per option: explicit argument > environment > .env > default.
/// database, user and password have no default and must come from one source.
/// An explicit None wins over the environment for options that accept None.
/// env_file=False skips the .env file but still reads FB_* variables.
/// Construction validates every value and never connects to the database.
///
/// Every option, its FB_* variable, default and effect:
/// https://github.com/rafael-peter/fbfast/blob/main/docs/guide/configuration.md
#[pyclass(name = "FirebirdConfig", module = "fbfast", frozen)]
pub struct PyConfig {
    pub inner: Config,
}

#[pymethods]
impl PyConfig {
    #[new]
    #[pyo3(signature = (**kwargs), text_signature = "(*, host=..., port=..., database=..., user=..., password=..., role=..., read_only=..., charset=..., session_time_zone=..., client_library=..., wire_crypt=..., wire_compression=..., app_name=..., pool_size=..., min_idle=..., max_waiters=..., acquire_timeout=..., connect_timeout=..., idle_timeout=..., max_lifetime=..., health_check_after=..., close_timeout=..., fetch_size=..., batch_bytes=..., prefetch_batches=..., max_result_bytes=..., max_result_rows=..., max_blob_bytes=..., query_timeout=..., lock_timeout=..., stream_idle_timeout=..., statement_cache_size=..., statement_cache_bytes=..., trim_char=..., fallback_charset=..., snapshot_timeout=..., max_inline_blob_size=..., blob_cache_size=..., diagnostics=..., ready_transaction_timeout=..., env_file=...)")]
    fn new(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let (explicit, env_file) = parse_kwargs(kwargs)?;
        Ok(Self {
            inner: Config::resolve(explicit, "FB_", env_file).map_err(from_core)?,
        })
    }

    /// Shortcut for a custom prefix (e.g. "FB_BI_"); FirebirdConfig() already reads FB_*.
    #[classmethod]
    #[pyo3(signature = (prefix = "FB_", **kwargs))]
    fn from_env(
        _class: &Bound<'_, PyType>,
        prefix: &str,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let (explicit, env_file) = parse_kwargs(kwargs)?;
        if !explicit.is_empty() {
            return Err(configuration("from_env only accepts prefix and env_file"));
        }
        Ok(Self {
            inner: Config::resolve(explicit, prefix, env_file).map_err(from_core)?,
        })
    }

    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        if name == "password" {
            return self.inner.password.expose().into_py_any(py);
        }
        let values = serde_json::to_value(&self.inner)
            .map_err(|_| configuration("cannot inspect configuration"))?;
        match values.get(name) {
            Some(Value::Null) => Ok(py.None()),
            Some(Value::Bool(b)) => b.into_py_any(py),
            Some(Value::String(s)) => s.into_py_any(py),
            Some(Value::Number(n)) => {
                if let Some(n) = n.as_u64() {
                    return n.into_py_any(py);
                }
                n.as_f64().into_py_any(py)
            }
            _ => Err(PyAttributeError::new_err(format!(
                "FirebirdConfig has no attribute {name}"
            ))),
        }
    }

    fn __repr__(&self) -> String {
        format!("FirebirdConfig({:?})", self.inner)
    }
}

fn parse_kwargs(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<(Inputs, EnvFile)> {
    let mut explicit = Inputs::new();
    let mut env_file = EnvFile::Default;
    if let Some(kwargs) = kwargs {
        for (key, value) in kwargs.iter() {
            let key: String = key.extract()?;
            if key == "env_file" {
                env_file = if value.is_instance_of::<PyBool>() && !value.extract::<bool>()? {
                    EnvFile::Disabled
                } else if let Ok(path) = value.extract::<String>() {
                    EnvFile::Path(path.into())
                } else {
                    return Err(configuration("env_file requires False or a path string"));
                };
            } else {
                explicit.insert(key.clone(), scalar(&key, &value)?);
            }
        }
    }
    Ok((explicit, env_file))
}

fn scalar(field: &str, value: &Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    if value.is_instance_of::<PyBool>() {
        return Ok(Value::Bool(value.extract()?));
    }
    if let Ok(v) = value.extract::<String>() {
        return Ok(Value::String(v));
    }
    if let Ok(v) = value.extract::<i64>() {
        return Ok(Value::Number(v.into()));
    }
    if let Ok(v) = value.extract::<u64>() {
        return Ok(Value::Number(v.into()));
    }
    if let Ok(v) = value.extract::<f64>() {
        return Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| configuration(format!("{field}: requires a finite number")));
    }
    Err(configuration(format!(
        "{field}: unsupported configuration value type"
    )))
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyConfig>()
}
