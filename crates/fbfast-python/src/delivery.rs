//! One conversion and delivery sequence for every materialized result, shared
//! by synchronous calls, coroutines and query groups.
use crate::{
    errors::from_core,
    executor::Executor,
    pool_wait::{internal, wait},
    values::Converter,
};
use fbfast_core::{
    pool::{Output, Pending, Query, Request},
    sinks::{arrow::ArrowMode, json::JsonOptions},
};
use pyo3::{prelude::*, types::PyList, IntoPyObjectExt};

/// What the caller asked for; decides the native request and the Python value.
#[derive(Clone, Copy)]
pub enum Form {
    Arrow(ArrowMode),
    Rows { first: bool, as_dict: bool },
    Value,
    Plan,
    Json(JsonOptions),
}

impl Form {
    pub fn request(self) -> Request {
        match self {
            Self::Arrow(mode) => Request::Arrow(mode),
            Self::Json(json) => Request::Json(json),
            Self::Plan => Request::Explain,
            Self::Value => Request::Rows {
                first: true,
                unique_names: false,
            },
            Self::Rows { first, as_dict } => Request::Rows {
                first,
                unique_names: as_dict,
            },
        }
    }
}

/// Builds the Python value. `per_row` converts rows one at a time so async
/// delivery can observe cancellation between rows.
pub fn convert(
    py: Python<'_>,
    converter: &Converter<'_>,
    form: Form,
    output: &Output,
    per_row: bool,
) -> PyResult<Py<PyAny>> {
    match (output, form) {
        (Output::Arrow(output), Form::Arrow(mode)) => crate::arrow::convert(py, output, mode),
        (Output::Json(json), Form::Json(_)) => converter.bytes(&json.data),
        (
            Output::Rows(rows),
            Form::Rows {
                first: true,
                as_dict,
            },
        ) => converter.row(rows, 0, as_dict),
        (Output::Rows(rows), Form::Rows { as_dict, .. }) if per_row => {
            let result = PyList::empty(py);
            for index in 0..rows.count {
                result.append(converter.row(rows, index, as_dict)?)?;
            }
            Ok(result.into_any().unbind())
        }
        (Output::Rows(rows), Form::Rows { as_dict, .. }) => converter.all(rows, as_dict),
        (Output::Rows(rows), Form::Value) => rows
            .row(0)
            .and_then(|row| row.first())
            .map_or_else(|| Ok(py.None()), |value| converter.value(value)),
        (Output::Plan(plan), Form::Plan) => plan.into_py_any(py),
        _ => Err(internal()),
    }
}

/// Converts a finished result inside the operation's deadline, frees native
/// buffers without the GIL and acknowledges delivery (releases snapshot leases).
pub fn finish(
    py: Python<'_>,
    converter: &mut Converter<'_>,
    pending: &Pending,
    form: Form,
    output: Output,
) -> PyResult<Py<PyAny>> {
    converter.set_budget(pending.delivery_budget().map_err(from_core)?);
    converter.checkpoint()?;
    let result = convert(py, converter, form, &output, false);
    py.detach(|| drop(output));
    let result = result?;
    converter.checkpoint()?;
    pending.delivery_complete();
    Ok(result)
}

/// Synchronous query: submit, wait with the GIL released, convert, acknowledge.
pub fn run(
    py: Python<'_>,
    executor: &Executor,
    sql: String,
    params: Option<&Bound<'_, PyAny>>,
    timeout: Option<f64>,
    form: Form,
) -> PyResult<Py<PyAny>> {
    executor.stats().map_err(from_core)?;
    let mut converter = Converter::new(py)?;
    let params = converter.params(params)?;
    let mut pending = executor
        .submit(Query::new(sql, params, timeout, form.request()))
        .map_err(from_core)?;
    let output = wait(py, &mut pending)?;
    finish(py, &mut converter, &pending, form, output)
}
