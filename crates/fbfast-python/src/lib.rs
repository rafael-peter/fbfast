use pyo3::prelude::*;

mod async_runtime;
mod asyncio;
mod config;
mod errors;
mod pool;
mod runtime;
mod values;

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    errors::register(module)?;
    config::register(module)?;
    pool::register(module)?;
    runtime::register(module)
}
