//! Owned record batches and schemas exportable without any Python Arrow package.
use arrow_array::{ffi::to_ffi, Array, RecordBatch, StructArray};
use arrow_schema::{ffi::FFI_ArrowSchema, SchemaRef};
use pyo3::{
    exceptions::{PyNotImplementedError, PyValueError},
    prelude::*,
    types::PyCapsule,
};

#[pyclass(name = "ArrowBatch", module = "fbfast", frozen)]
pub struct Batch {
    pub batch: RecordBatch,
}

#[pyclass(name = "ArrowSchema", module = "fbfast", frozen)]
pub struct Schema {
    pub schema: SchemaRef,
}

pub fn requested(value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
    if value.is_some_and(|v| !v.is_none()) {
        Err(PyNotImplementedError::new_err(
            "Arrow schema overrides are not supported",
        ))
    } else {
        Ok(())
    }
}

#[pymethods]
impl Batch {
    #[getter]
    fn num_rows(&self) -> usize {
        self.batch.num_rows()
    }
    #[getter]
    fn num_columns(&self) -> usize {
        self.batch.num_columns()
    }
    #[getter]
    fn nbytes(&self) -> usize {
        self.batch.get_array_memory_size()
    }
    fn __len__(&self) -> usize {
        self.batch.num_rows()
    }
    #[getter]
    fn schema(&self) -> Schema {
        Schema {
            schema: self.batch.schema(),
        }
    }
    #[pyo3(signature = (requested_schema=None))]
    fn __arrow_c_array__<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<(Bound<'py, PyCapsule>, Bound<'py, PyCapsule>)> {
        requested(requested_schema)?;
        let array = StructArray::from(self.batch.clone());
        let (array, _) =
            to_ffi(&array.to_data()).map_err(|e| PyValueError::new_err(e.to_string()))?;
        // Export the full RecordBatch schema so SQL field metadata is retained.
        let schema = FFI_ArrowSchema::try_from(self.batch.schema().as_ref())
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok((
            PyCapsule::new(py, schema, Some(c"arrow_schema".into()))?,
            PyCapsule::new(py, array, Some(c"arrow_array".into()))?,
        ))
    }
}
#[pymethods]
impl Schema {
    #[getter]
    fn names(&self) -> Vec<String> {
        self.schema
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect()
    }
    fn __len__(&self) -> usize {
        self.schema.fields().len()
    }
    fn __arrow_c_schema__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyCapsule>> {
        let schema = FFI_ArrowSchema::try_from(self.schema.as_ref())
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        PyCapsule::new(py, schema, Some(c"arrow_schema".into()))
    }
}
pub fn convert(py: Python<'_>, batch: &RecordBatch) -> PyResult<Py<PyAny>> {
    Ok(Bound::new(
        py,
        Batch {
            batch: batch.clone(),
        },
    )?
    .into_any()
    .unbind())
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Batch>()?;
    module.add_class::<Schema>()
}
