//! Pool statistics and diagnostic records as plain Python data.
use crate::pool_wait::internal;
use fbfast_core::{
    execution::{QueryTrace, SessionInfo},
    pool::Stats,
};
use pyo3::{
    prelude::*,
    types::{PyDict, PyList},
    IntoPyObjectExt,
};
use serde_json::Value;

pub fn stats(py: Python<'_>, stats: Stats) -> PyResult<Bound<'_, PyDict>> {
    let values = serde_json::to_value(stats).map_err(|_| internal())?;
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

pub fn diagnostics(py: Python<'_>, records: Vec<QueryTrace>) -> PyResult<Bound<'_, PyList>> {
    let values = serde_json::to_value(records).map_err(|_| internal())?;
    let list = PyList::empty(py);
    for record in values.as_array().ok_or_else(internal)? {
        list.append(json_value(py, record)?)?;
    }
    Ok(list)
}

pub fn session_info(py: Python<'_>, info: Option<SessionInfo>) -> PyResult<Py<PyAny>> {
    match info {
        None => Ok(py.None()),
        Some(info) => json_value(py, &serde_json::to_value(info).map_err(|_| internal())?),
    }
}

/// Plain JSON data (numbers, text, booleans, lists, objects) as Python values.
fn json_value(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(flag) => flag.into_py_any(py),
        Value::Number(number) => match number.as_u64() {
            Some(integer) => integer.into_py_any(py),
            None => number.as_f64().ok_or_else(internal)?.into_py_any(py),
        },
        Value::String(text) => text.into_py_any(py),
        Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(json_value(py, item)?)?;
            }
            list.into_py_any(py)
        }
        Value::Object(fields) => {
            let dict = PyDict::new(py);
            for (name, item) in fields {
                dict.set_item(name, json_value(py, item)?)?;
            }
            dict.into_py_any(py)
        }
    }
}
