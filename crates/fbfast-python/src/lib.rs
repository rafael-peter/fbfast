use pyo3::prelude::*;

mod arrow;
mod arrow_batch;
mod arrow_stream;
mod async_runtime;
mod asyncio;
mod config;
mod errors;
mod executor;
mod json;
mod pool;
mod pool_wait;
mod runtime;
mod stream;
mod values;

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    errors::register(module)?;
    config::register(module)?;
    pool::register(module)?;
    arrow::register(module)?;
    arrow_batch::register(module)?;
    runtime::register(module)
}
