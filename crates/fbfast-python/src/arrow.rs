//! Ownership transfer through standard Arrow PyCapsules, with no Python rows.
use crate::{
    errors::from_core,
    pool::PyPool,
    pool_wait::{internal, wait},
    values::Converter,
};
use arrow_array::{ffi_stream::FFI_ArrowArrayStream, RecordBatchIterator};
use arrow_schema::DataType;
use fbfast_core::{
    error::Error,
    pool::Output,
    sinks::arrow::{ArrowMode, ArrowOutput},
};
use pyo3::{exceptions::PyNotImplementedError, prelude::*, types::PyCapsule};
use serde_json::{json, Value};
use std::sync::Arc;

#[pyclass(name = "_ArrowTable", module = "fbfast._native", frozen)]
pub struct Table {
    output: Arc<ArrowOutput>,
}

#[pymethods]
impl Table {
    #[pyo3(signature = (requested_schema=None))]
    fn __arrow_c_stream__<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyCapsule>> {
        if requested_schema.is_some_and(|value| !value.is_none()) {
            return Err(PyNotImplementedError::new_err(
                "Arrow schema overrides are not supported",
            ));
        }
        let reader = RecordBatchIterator::new(
            self.output.batches.clone().into_iter().map(Ok),
            self.output.schema.clone(),
        );
        // PyO3 owns the ABI struct. Its Drop invokes Arrow's release callback
        // only if still live; consumers move it and null the original release.
        PyCapsule::new_with_value(
            py,
            FFI_ArrowArrayStream::new(Box::new(reader)),
            c"arrow_array_stream",
        )
    }
    #[getter]
    fn schema_spec(&self) -> String {
        serde_json::to_string(
            &self
                .output
                .schema
                .fields()
                .iter()
                .map(|field| json!({"name": field.name(), "type": dtype(field.data_type())}))
                .collect::<Vec<_>>(),
        )
        .expect("finite schema")
    }
}

fn dtype(value: &DataType) -> Value {
    use DataType::*;
    match value {
        Boolean => json!({"kind": "Boolean"}),
        Int16 => json!({"kind": "Int16"}),
        Int32 => json!({"kind": "Int32"}),
        Int64 => json!({"kind": "Int64"}),
        Float32 => json!({"kind": "Float32"}),
        Float64 => json!({"kind": "Float64"}),
        LargeUtf8 => json!({"kind": "String"}),
        LargeBinary => json!({"kind": "Binary"}),
        Date32 => json!({"kind": "Date"}),
        Time64(_) => json!({"kind": "Time"}),
        Timestamp(_, zone) => json!({"kind": "Datetime", "zone": zone.as_deref()}),
        Decimal128(precision, scale) => {
            json!({"kind": "Decimal", "precision": precision, "scale": scale})
        }
        Struct(fields) => json!({"kind": "Struct", "fields": fields.iter().map(|field|
            json!({"name": field.name(), "type": dtype(field.data_type())})).collect::<Vec<_>>()}),
        _ => json!({"kind": "Unsupported"}),
    }
}

pub fn mode(value: &str) -> PyResult<ArrowMode> {
    match value {
        "arrow" => Ok(ArrowMode::Native),
        "polars" => Ok(ArrowMode::Polars),
        "polars_string" => Ok(ArrowMode::PolarsString),
        _ => Err(from_core(Error::config(
            "arrow_mode",
            "invalid analytical output mode",
        ))),
    }
}

pub fn convert(py: Python<'_>, output: &ArrowOutput, mode: ArrowMode) -> PyResult<Py<PyAny>> {
    let table = Table {
        output: Arc::new(ArrowOutput {
            schema: output.schema.clone(),
            batches: output.batches.clone(),
            rows: output.rows,
            bytes: output.bytes,
        }),
    };
    let function = py.import("fbfast.analytics")?.getattr("_convert")?;
    Ok(function
        .call1((Bound::new(py, table)?, matches!(mode, ArrowMode::Native)))?
        .unbind())
}

pub fn query<'py>(
    py: Python<'py>,
    pool: &PyPool,
    sql: String,
    params: Option<&Bound<'py, PyAny>>,
    mode: ArrowMode,
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
            crate::asyncio::Form::Arrow(mode),
        )
        .map(Bound::unbind);
    }
    pool.inner.stats().map_err(from_core)?;
    let mut converter = Converter::new(py)?;
    let params = converter.params(params)?;
    let mut pending = pool
        .inner
        .submit_arrow(sql, params, mode, timeout)
        .map_err(from_core)?;
    let output = wait(py, &mut pending)?;
    converter.set_budget(pending.delivery_budget().map_err(from_core)?);
    converter.checkpoint()?;
    let result = match &output {
        Output::Arrow(output) => convert(py, output, mode),
        _ => Err(internal()),
    };
    py.detach(|| drop(output));
    let result = result?;
    converter.checkpoint()?;
    pending.delivery_complete();
    Ok(result)
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Table>()
}
