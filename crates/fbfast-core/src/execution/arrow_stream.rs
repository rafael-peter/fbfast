use super::{
    control::QueryOptions, cursor::RowSink, BatchConsumer, ExecutionConnection, StreamSummary,
};
use crate::{
    config::Config,
    decode::{DecodePlan, NativeReader, Value},
    error::Error,
    sinks::arrow::{ArrowBuilder, ArrowMode},
};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;

pub trait ArrowConsumer: BatchConsumer<RecordBatch> {
    /// Schema precedes demand and exists even for an empty result.
    fn schema(&mut self, schema: SchemaRef) -> Result<(), Error>;
}

struct Batches<'a, C> {
    builder: ArrowBuilder,
    consumer: &'a mut C,
    summary: StreamSummary,
    announced: bool,
    capacity: usize,
    bytes: usize,
}
impl<C: ArrowConsumer> RowSink for Batches<'_, C> {
    fn before_fetch(&mut self) -> Result<bool, Error> {
        if !self.announced {
            self.consumer.schema(self.builder.schema.clone())?;
            self.announced = true;
        }
        self.consumer.before_fetch()
    }
    fn push(
        &mut self,
        plan: &DecodePlan,
        row: &[u8],
        _: &Config,
        native: &mut impl NativeReader,
    ) -> Result<(), Error> {
        self.builder.push(plan, row, native)
    }
    fn after_fetch(&mut self) -> Result<bool, Error> {
        if self.builder.rows == 0 {
            return Ok(true);
        }
        let batch = self.builder.finish()?;
        self.summary.rows += batch.num_rows() as u64;
        self.summary.bytes += batch.get_array_memory_size() as u64;
        self.summary.batches += 1;
        self.builder =
            ArrowBuilder::with_schema(self.builder.schema.clone(), self.capacity, self.bytes)?;
        self.consumer.batch(batch)
    }
}
impl ExecutionConnection {
    pub fn stream_arrow_with(
        &mut self,
        sql: &str,
        params: &[Value],
        batch_rows: u32,
        options: &QueryOptions<'_>,
        consumer: &mut impl ArrowConsumer,
    ) -> Result<StreamSummary, Error> {
        if batch_rows == 0 {
            return Err(Error::config("batch_rows", "must be positive"));
        }
        self.operation(sql, options, |connection, entry, deadline| {
            let bytes = connection.config.batch_bytes;
            let capacity = if entry.plan.has_blobs() {
                1
            } else {
                batch_rows.min(
                    (bytes / entry.plan.row_capacity_bytes()?.max(1) as u64)
                        .clamp(1, u32::MAX as u64) as u32,
                )
            };
            let mut sink = Batches {
                builder: ArrowBuilder::new(
                    &entry.plan,
                    &connection.config,
                    ArrowMode::Native,
                    capacity as usize,
                )?,
                consumer,
                summary: StreamSummary::default(),
                announced: false,
                capacity: capacity as usize,
                bytes: bytes as usize,
            };
            connection.pump(entry, params, options, deadline, capacity, &mut sink)?;
            Ok(sink.summary)
        })
    }
}
