//! Blocking C Stream bridge. One export owns consumption; context owns cleanup.
use crate::{arrow_batch, errors::from_core, stream::PyStream};
use arrow_array::{ffi_stream::FFI_ArrowArrayStream, RecordBatch, RecordBatchReader};
use arrow_schema::{ArrowError, SchemaRef};
use fbfast_core::{
    error::{Error, ErrorKind},
    pool::{Stream, StreamAction, StreamItem},
};
use pyo3::{prelude::*, types::PyCapsule};
use std::{fmt, sync::Arc, time::Duration};

pub fn busy() -> Error {
    Error::new(
        ErrorKind::PoolBusy,
        "arrow_stream",
        "Arrow C Stream already owns consumption",
    )
}

pub fn export<'py>(
    stream: &PyStream,
    py: Python<'py>,
    requested: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyCapsule>> {
    stream.core.check_pid().map_err(from_core)?;
    arrow_batch::requested(requested)?;
    let mut exported = stream.exported.lock().unwrap_or_else(|e| e.into_inner());
    if *exported {
        return Err(from_core(busy()));
    }
    stream.core.check_idle().map_err(from_core)?;
    let schema = stream.core.arrow_schema().map_err(from_core)?;
    let reader = Reader {
        core: stream.core.clone(),
        schema,
        ended: false,
    };
    let capsule = PyCapsule::new_with_value(
        py,
        FFI_ArrowArrayStream::new(Box::new(reader)),
        c"arrow_array_stream",
    )?;
    *exported = true;
    Ok(capsule)
}

struct Reader {
    core: Arc<Stream>,
    schema: SchemaRef,
    ended: bool,
}
impl Reader {
    fn read(&mut self) -> Result<Option<RecordBatch>, ArrowError> {
        let mut wait = self.core.request(StreamAction::Next).map_err(foreign)?;
        loop {
            // Foreign consumers may call with or without the GIL. Release it
            // while waiting, and honor Python's existing main-thread handlers.
            let item = Python::try_attach(|py| {
                py.check_signals()
                    .map_err(|e| ArrowError::ExternalError(Box::new(e)))?;
                py.detach(|| {
                    let item = wait.poll().map_err(foreign)?;
                    if item.is_none() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok::<_, ArrowError>(item)
                })
            })
            .ok_or_else(|| {
                ArrowError::CDataInterface(
                    "Python interpreter unavailable during Arrow consumption".into(),
                )
            })??;
            match item {
                None => (),
                Some(StreamItem::Arrow(batch)) => return Ok(Some(batch)),
                Some(StreamItem::End) => break,
                _ => {
                    return Err(foreign(Error::new(
                        ErrorKind::Query,
                        "arrow_stream",
                        "Unexpected Arrow stream event",
                    )))
                }
            }
        }
        // Release the exclusive waiter before acknowledging successful cleanup.
        drop(wait);
        let mut close = self.core.request(StreamAction::Close).map_err(foreign)?;
        close.poll().map_err(foreign)?;
        Ok(None)
    }
}
impl Iterator for Reader {
    type Item = Result<RecordBatch, ArrowError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.ended {
            return None;
        }
        match self.read() {
            Ok(Some(batch)) => Some(Ok(batch)),
            Ok(None) => {
                self.ended = true;
                None
            }
            Err(error) => {
                self.ended = true;
                Some(Err(error))
            }
        }
    }
}
impl RecordBatchReader for Reader {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        // No blocking join in a foreign release callback. The surrounding
        // context waits for cleanup; if abandoned, Stream::drop cancels it.
        let _ = self.core.stop(false);
    }
}

#[derive(Debug)]
struct Failure(Error);
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let e = &self.0;
        write!(
            f,
            "fbfast {:?} during {}: {} (SQLSTATE {:?}, native codes {:?})",
            e.kind, e.phase, e.message, e.sqlstate, e.native_codes
        )?;
        for cleanup in &e.cleanup {
            write!(f, "; cleanup: {}", Failure(cleanup.clone()))?;
        }
        Ok(())
    }
}
impl std::error::Error for Failure {}
fn foreign(error: Error) -> ArrowError {
    ArrowError::ExternalError(Box::new(Failure(error)))
}
