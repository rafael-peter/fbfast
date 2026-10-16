use super::{control::QueryOptions, cursor::RowSink, ExecutionConnection};
use crate::{
    config::Config,
    decode::{DecodePlan, NativeReader, Value},
    error::Error,
    sinks::rows::Rows,
};

#[derive(Default, Debug)]
pub struct StreamSummary {
    pub rows: u64,
    pub bytes: u64,
    pub batches: u64,
}

/// Worker-side consumer. Waiting here must be bounded and cancellation-aware;
/// no Python callbacks or objects belong in this path.
pub trait BatchConsumer<B = Rows> {
    fn before_fetch(&mut self) -> Result<bool, Error> {
        Ok(true)
    }
    /// false closes early, preserving normal cursor/transaction cleanup.
    fn batch(&mut self, rows: B) -> Result<bool, Error>;
}

impl<B, F: FnMut(B) -> Result<bool, Error>> BatchConsumer<B> for F {
    fn batch(&mut self, rows: B) -> Result<bool, Error> {
        self(rows)
    }
}

struct Batches<'a, C> {
    rows: Rows,
    config: Config,
    consumer: &'a mut C,
    summary: StreamSummary,
}

impl<C: BatchConsumer> RowSink for Batches<'_, C> {
    fn before_fetch(&mut self) -> Result<bool, Error> {
        self.consumer.before_fetch()
    }
    fn push(
        &mut self,
        plan: &DecodePlan,
        row: &[u8],
        _: &Config,
        native: &mut impl NativeReader,
    ) -> Result<(), Error> {
        self.rows.push(plan, row, &self.config, native)
    }
    fn after_fetch(&mut self) -> Result<bool, Error> {
        if self.rows.count == 0 {
            return Ok(true);
        }
        let empty = Rows {
            names: self.rows.names.clone(),
            cells: Vec::new(),
            count: 0,
            bytes: 0,
        };
        let rows = std::mem::replace(&mut self.rows, empty);
        self.summary.rows += rows.count as u64;
        self.summary.bytes += rows.bytes as u64;
        self.summary.batches += 1;
        self.consumer.batch(rows)
    }
}

impl ExecutionConnection {
    pub fn stream_with(
        &mut self,
        sql: &str,
        params: &[Value],
        batch_rows: u32,
        options: &QueryOptions<'_>,
        consumer: &mut impl BatchConsumer,
    ) -> Result<StreamSummary, Error> {
        if batch_rows == 0 {
            return Err(Error::config("batch_rows", "must be positive"));
        }
        self.operation(sql, options, |connection, entry, deadline| {
            let rows = Rows::new(&entry.plan);
            if options.unique_names {
                rows.require_unique_names()?;
            }
            let row_bytes = entry.plan.row_capacity_bytes()?.max(1) as u64;
            let byte_rows =
                (connection.config.batch_bytes / row_bytes).clamp(1, u32::MAX as u64) as u32;
            let batch_rows = batch_rows.min(byte_rows);
            // Streaming does not apply the cumulative materialization quota.
            // Native batch/cell bounds still apply; the caller owns retained batches.
            let mut config = connection.config.clone();
            config.max_result_rows = None;
            config.max_result_bytes = None;
            let mut sink = Batches {
                rows,
                config,
                consumer,
                summary: StreamSummary::default(),
            };
            connection.pump(entry, params, options, deadline, batch_rows, &mut sink)?;
            Ok(sink.summary)
        })
    }
}
