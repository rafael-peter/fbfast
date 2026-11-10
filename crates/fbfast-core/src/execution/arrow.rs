use super::{
    control::QueryOptions,
    cursor::{native_batch_rows, RowSink},
    ExecutionConnection,
};
use crate::{
    config::Config,
    decode::{DecodePlan, NativeReader, Value},
    error::{Error, ErrorKind},
    sinks::arrow::{ArrowBuilder, ArrowMode, ArrowOutput},
    sinks::limits::add_materialized_bytes,
};

struct Materialized {
    builder: ArrowBuilder,
    output: ArrowOutput,
    capacity: usize,
    bytes: usize,
    max_result_bytes: Option<u64>,
}
impl RowSink for Materialized {
    fn push(
        &mut self,
        plan: &DecodePlan,
        row: &[u8],
        config: &Config,
        native: &mut impl NativeReader,
    ) -> Result<(), Error> {
        if config
            .max_result_rows
            .is_some_and(|limit| (self.output.rows + self.builder.rows) as u64 >= limit)
        {
            return Err(Error::new(
                ErrorKind::ResultLimit,
                "materialize",
                "Materialized result exceeds max_result_rows",
            ));
        }
        self.builder.push(plan, row, native)
    }
    fn pending_bytes(&self) -> usize {
        self.builder.pending_bytes
    }
    fn after_fetch(&mut self) -> Result<bool, Error> {
        if self.builder.rows != 0 {
            let batch = self.builder.finish()?;
            let rows = self
                .output
                .rows
                .checked_add(batch.num_rows())
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::ResultLimit,
                        "materialize",
                        "Materialized result row counter overflow",
                    )
                })?;
            let bytes = add_materialized_bytes(
                self.output.bytes,
                batch.get_array_memory_size(),
                self.max_result_bytes,
            )?;
            self.output.rows = rows;
            self.output.bytes = bytes;
            self.output.batches.push(batch);
            self.builder =
                ArrowBuilder::with_schema(self.output.schema.clone(), self.capacity, self.bytes)?;
        }
        Ok(true)
    }
}

impl ExecutionConnection {
    pub fn arrow_with(
        &mut self,
        sql: &str,
        params: &[Value],
        mode: ArrowMode,
        options: &QueryOptions<'_>,
    ) -> Result<ArrowOutput, Error> {
        self.operation(sql, options, |connection, entry, deadline| {
            let capacity = native_batch_rows(
                &entry.plan,
                connection.config.fetch_size,
                connection.config.batch_bytes,
            )?;
            let builder =
                ArrowBuilder::new(&entry.plan, &connection.config, mode, capacity as usize)?;
            let output = ArrowOutput {
                schema: builder.schema.clone(),
                batches: Vec::new(),
                rows: 0,
                bytes: 0,
            };
            let mut sink = Materialized {
                builder,
                output,
                capacity: capacity as usize,
                bytes: connection.config.batch_bytes as usize,
                max_result_bytes: connection.config.max_result_bytes,
            };
            connection.pump(entry, params, options, deadline, capacity, &mut sink)?;
            Ok(sink.output)
        })
    }
}
