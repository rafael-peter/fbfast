use pyo3::{exceptions::PyRuntimeError, prelude::*, types::PyDict};
use std::path::PathBuf;

#[pyfunction]
fn compiled_info() -> PyResult<String> {
    serde_json::to_string(&fbfast_core::build_info::build_info())
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))
}

#[pyfunction]
fn load_client(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let client = py
        .detach(|| fbfast_core::runtime::client_info(&path))
        .map_err(crate::errors::from_core)?;
    let result = PyDict::new(py);
    result.set_item("path", client.path.to_string_lossy().as_ref())?;
    result.set_item("version", client.version)?;
    result.set_item("version_string", client.version_string)?;
    result.set_item("root", client.root.to_string_lossy().as_ref())?;
    Ok(result)
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(compiled_info, module)?)?;
    module.add_function(wrap_pyfunction!(load_client, module)?)?;
    Ok(())
}
