use crate::errors::{from_core, ConfigurationError};
use fbfast_core::config::{Config, EnvFile, Inputs};
use pyo3::{
    exceptions::PyAttributeError,
    prelude::*,
    types::{PyBool, PyDict, PyType},
    IntoPyObjectExt,
};
use serde_json::{Number, Value};

#[pyclass(name = "FirebirdConfig", module = "fbfast", frozen)]
pub struct PyConfig {
    pub inner: Config,
}

#[pymethods]
impl PyConfig {
    #[new]
    #[pyo3(signature = (**kwargs))]
    fn new(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let (explicit, env_file) = parse_kwargs(kwargs)?;
        Ok(Self {
            inner: Config::resolve(explicit, "FB_", env_file).map_err(from_core)?,
        })
    }

    #[classmethod]
    #[pyo3(signature = (prefix = "FB_", **kwargs))]
    fn from_env(
        _class: &Bound<'_, PyType>,
        prefix: &str,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let (explicit, env_file) = parse_kwargs(kwargs)?;
        if !explicit.is_empty() {
            return Err(ConfigurationError::new_err(
                "from_env only accepts prefix and env_file",
            ));
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
            .map_err(|_| ConfigurationError::new_err("cannot inspect configuration"))?;
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
                    return Err(ConfigurationError::new_err(
                        "env_file requires False or a path string",
                    ));
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
        return Number::from_f64(v).map(Value::Number).ok_or_else(|| {
            ConfigurationError::new_err(format!("{field}: requires a finite number"))
        });
    }
    Err(ConfigurationError::new_err(format!(
        "{field}: unsupported configuration value type"
    )))
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyConfig>()
}
