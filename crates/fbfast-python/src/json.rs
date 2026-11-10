use crate::{errors::from_core, pool::PyPool};
use fbfast_core::{
    error::Error,
    sinks::json::{JsonFormat, JsonOptions},
};
use pyo3::prelude::*;

pub fn options(values: &[String]) -> PyResult<JsonOptions> {
    if values.len() != 5 {
        return Err(from_core(Error::config(
            "json_options",
            "expected five options",
        )));
    }
    let choice = |index: usize, name: &str, first: &str, second: &str| match values[index].as_str()
    {
        value if value == first => Ok(true),
        value if value == second => Ok(false),
        _ => Err(from_core(Error::config(
            name,
            &format!("expected '{first}' or '{second}'"),
        ))),
    };
    Ok(JsonOptions {
        records: choice(0, "orient", "records", "values")?,
        safe_integers: choice(1, "json_integers", "safe", "number")?,
        string_decimals: choice(2, "json_decimals", "string", "number")?,
        base64_binary: choice(3, "json_binary", "base64", "error")?,
        null_non_finite: choice(4, "json_non_finite", "null", "error")?,
    })
}
pub fn format(value: &str) -> PyResult<JsonFormat> {
    match value {
        "array" => Ok(JsonFormat::Array),
        "ndjson" => Ok(JsonFormat::Ndjson),
        _ => Err(from_core(Error::config(
            "format",
            "expected 'array' or 'ndjson'",
        ))),
    }
}
pub fn query<'py>(
    py: Python<'py>,
    pool: &PyPool,
    sql: String,
    params: Option<&Bound<'py, PyAny>>,
    json: JsonOptions,
    timeout: Option<f64>,
    asynchronous: bool,
) -> PyResult<Py<PyAny>> {
    if asynchronous {
        return crate::asyncio::query(
            py,
            &pool.inner,
            &pool.delivery,
            sql,
            params,
            timeout,
            crate::asyncio::Form::Json(json),
        )
        .map(Bound::unbind);
    }
    crate::delivery::run(
        py,
        &pool.inner,
        sql,
        params,
        timeout,
        crate::delivery::Form::Json(json),
    )
}
