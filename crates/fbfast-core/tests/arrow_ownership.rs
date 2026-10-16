#![forbid(unsafe_code)]
//! Exercise the same Arrow ownership boundary used by the Python capsule owner.
use arrow_array::{
    ffi_stream::{ArrowArrayStreamReader, FFI_ArrowArrayStream},
    Array, Int32Array, RecordBatch, RecordBatchReader,
};
use arrow_schema::{ArrowError, SchemaRef};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct Reader {
    batch: Option<RecordBatch>,
    schema: SchemaRef,
    released: Arc<AtomicUsize>,
}
impl Iterator for Reader {
    type Item = Result<RecordBatch, ArrowError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.batch.take().map(Ok)
    }
}
impl RecordBatchReader for Reader {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.released.fetch_add(1, Ordering::SeqCst);
    }
}

fn export(released: &Arc<AtomicUsize>) -> (FFI_ArrowArrayStream, usize) {
    let values = Int32Array::from(vec![Some(i32::MIN), None, Some(i32::MAX)]);
    let address = values.values().as_ptr() as usize;
    let batch = RecordBatch::try_from_iter([("i", Arc::new(values) as _)]).unwrap();
    let reader = Reader {
        schema: batch.schema(),
        batch: Some(batch),
        released: released.clone(),
    };
    (FFI_ArrowArrayStream::new(Box::new(reader)), address)
}

#[test]
fn unconsumed_export_releases_its_reader_once() {
    let releases = Arc::new(AtomicUsize::new(0));
    let (stream, _) = export(&releases);
    assert_eq!(releases.load(Ordering::SeqCst), 0);
    drop(stream);
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[test]
fn imported_batch_shares_buffers_and_outlives_stream_without_double_release() {
    let releases = Arc::new(AtomicUsize::new(0));
    let (stream, address) = export(&releases);
    let mut consumer = ArrowArrayStreamReader::try_new(stream).unwrap();
    let batch = consumer.next().unwrap().unwrap();
    assert!(consumer.next().is_none());
    assert_eq!(releases.load(Ordering::SeqCst), 0);
    drop(consumer);
    assert_eq!(releases.load(Ordering::SeqCst), 1);
    let array = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int32Array>()
        .unwrap();
    assert_eq!(array.values().as_ptr() as usize, address);
    assert_eq!(array.value(0), i32::MIN);
    assert!(array.is_null(1));
    assert_eq!(array.value(2), i32::MAX);
    drop(batch);
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[test]
fn empty_or_released_stream_is_rejected_before_callback_use() {
    assert!(ArrowArrayStreamReader::try_new(FFI_ArrowArrayStream::empty()).is_err());
}

#[test]
fn unconsumed_c_array_keeps_and_releases_its_buffer_reference() {
    let data = Int32Array::from(vec![Some(1), None, Some(3)]).to_data();
    let observer = data.buffers()[0].clone();
    let before = observer.strong_count();
    let (array, schema) = arrow_array::ffi::to_ffi(&data).unwrap();
    assert!(observer.strong_count() > before);
    drop(data);
    assert!(observer.strong_count() > 1);
    drop(array);
    drop(schema);
    assert_eq!(observer.strong_count(), 1);
}
