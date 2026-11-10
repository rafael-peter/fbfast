use super::{control::QueryOptions, ExecutionConnection};
use crate::{
    config::Config,
    decode::{bind, DecodePlan, NativeReader, Value},
    error::{Error, ErrorKind},
    sinks::rows::Rows,
    statements::cache::Entry,
};
use std::time::Instant;

/// Statically dispatched row sinks share native cursor/binding/deadline logic.
pub(super) trait RowSink {
    fn before_fetch(&mut self) -> Result<bool, Error> {
        Ok(true)
    }
    fn push(
        &mut self,
        plan: &DecodePlan,
        row: &[u8],
        config: &Config,
        native: &mut impl NativeReader,
    ) -> Result<(), Error>;
    fn after_fetch(&mut self) -> Result<bool, Error> {
        Ok(true)
    }
    /// Decoded bytes held for the batch being built (0: no batch structure).
    fn pending_bytes(&self) -> usize {
        0
    }
}

/// The one batch planner: the requested rows, bounded by `batch_bytes` over
/// the decoded size of one row. BLOB cells are only IDs in the wire message, so
/// `pump` additionally splits a batch once its decoded bytes reach the budget.
pub(super) fn native_batch_rows(
    plan: &DecodePlan,
    requested: u32,
    batch_bytes: u64,
) -> Result<u32, Error> {
    let row = plan.row_capacity_bytes()?.max(1) as u64;
    Ok(requested.min((batch_bytes / row).clamp(1, u32::MAX as u64) as u32))
}

impl RowSink for Rows {
    fn push(
        &mut self,
        plan: &DecodePlan,
        row: &[u8],
        config: &Config,
        native: &mut impl NativeReader,
    ) -> Result<(), Error> {
        self.push(plan, row, config, native)
    }
}

impl ExecutionConnection {
    pub(super) fn pump(
        &mut self,
        entry: &Entry,
        params: &[Value],
        options: &QueryOptions<'_>,
        deadline: Option<Instant>,
        batch_rows: u32,
        sink: &mut impl RowSink,
    ) -> Result<(), Error> {
        let parameters = bind::encode_with(&entry.input, params, &mut |kind, value| {
            crate::decode::encode_native(&mut self.native, kind, value)
        })?;
        options.cancellation.check()?;
        check_deadline(deadline)?;
        let native_timeout = deadline
            .map(|end| {
                end.saturating_duration_since(Instant::now())
                    .as_millis()
                    .clamp(1, u32::MAX as u128) as u32
            })
            .unwrap_or(0);
        self.native
            .open_cursor(entry.id, &parameters, native_timeout)?;
        let batch_rows = native_batch_rows(&entry.plan, batch_rows, self.config.batch_bytes)?;
        // BLOB IDs stay valid for the whole transaction, so several rows can be
        // fetched before their BLOBs are read. Decoded BLOBs can be large: hand
        // over a batch as soon as its decoded bytes reach batch_bytes.
        let split = entry
            .plan
            .has_blobs()
            .then_some(self.config.batch_bytes as usize);
        let mut first_batch = true;
        'fetch: loop {
            options.cancellation.check()?;
            check_deadline(deadline)?;
            if !sink.before_fetch()? {
                break;
            }
            options.cancellation.check()?;
            check_deadline(deadline)?;
            let batch = self
                .native
                .fetch(batch_rows, self.config.batch_bytes as usize)?;
            if std::mem::take(&mut first_batch) {
                self.mark("execute");
            }
            for index in 0..batch.len() {
                let row = batch.row(index).ok_or_else(|| {
                    Error::new(
                        ErrorKind::DataConversion,
                        "fetch",
                        "Native batch index invalid",
                    )
                })?;
                options.cancellation.check()?;
                let mut native = super::decoder::NativeValues {
                    native: &mut self.native,
                    cancellation: options.cancellation,
                    deadline,
                    limit: self.config.max_blob_bytes,
                };
                sink.push(&entry.plan, row, &self.config, &mut native)?;
                check_deadline(deadline)?;
                let full = split.is_some_and(|limit| sink.pending_bytes() >= limit);
                if full
                    && index + 1 < batch.len()
                    && !(sink.after_fetch()? && sink.before_fetch()?)
                {
                    break 'fetch;
                }
            }
            let eof = batch.eof;
            self.drained |= eof;
            self.native.recycle(batch);
            if !sink.after_fetch()? || options.first || eof {
                break;
            }
        }
        self.mark("fetch");
        Ok(())
    }
}

pub(super) fn check_deadline(deadline: Option<Instant>) -> Result<(), Error> {
    if deadline.is_some_and(|end| Instant::now() >= end) {
        return Err(Error::new(
            ErrorKind::QueryTimeout,
            "query",
            "Query deadline expired",
        ));
    }
    Ok(())
}
