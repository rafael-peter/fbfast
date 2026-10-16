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
        // Consume every BLOB before advancing its row. This also bounds decoded
        // row streams to one variable-size row per batch, independently of the
        // small wire message containing only IDs. Client wire buffering remains.
        let batch_rows = if entry.plan.has_blobs() {
            1
        } else {
            batch_rows
        };
        loop {
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
            }
            if !sink.after_fetch()? || options.first || batch.eof {
                break;
            }
        }
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
