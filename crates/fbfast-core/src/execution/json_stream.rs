use super::{
    control::QueryOptions,
    cursor::{check_deadline, RowSink},
    BatchConsumer, ExecutionConnection, StreamSummary,
};
use crate::{
    config::Config,
    decode::{DecodePlan, NativeReader, Value},
    error::Error,
    sinks::json::{JsonFormat, JsonOptions, JsonOutput, JsonWriter},
};

struct Chunks<'a, C> {
    writer: JsonWriter,
    record: Vec<u8>,
    output: JsonOutput,
    limit: usize,
    format: JsonFormat,
    consumer: &'a mut C,
    summary: StreamSummary,
    stopped: bool,
}
impl<C: BatchConsumer<JsonOutput>> Chunks<'_, C> {
    fn flush(&mut self) -> Result<(), Error> {
        if self.output.data.is_empty() || self.stopped {
            return Ok(());
        }
        if !self.consumer.before_fetch()? {
            self.stopped = true;
            return Ok(());
        }
        let data = std::mem::replace(
            &mut self.output,
            JsonOutput {
                data: Vec::new(),
                rows: 0,
            },
        );
        self.summary.bytes += data.data.len() as u64;
        self.summary.batches += 1;
        if !self.consumer.batch(data)? {
            self.stopped = true;
        }
        Ok(())
    }
    fn append_array(&mut self, data: &[u8]) -> Result<(), Error> {
        let mut offset = 0;
        while offset < data.len() && !self.stopped {
            let take = (self.limit - self.output.data.len()).min(data.len() - offset);
            self.output
                .data
                .extend_from_slice(&data[offset..offset + take]);
            offset += take;
            if self.output.data.len() == self.limit {
                self.flush()?;
            }
        }
        Ok(())
    }
}
impl<C: BatchConsumer<JsonOutput>> RowSink for Chunks<'_, C> {
    fn before_fetch(&mut self) -> Result<bool, Error> {
        if self.stopped {
            return Ok(false);
        }
        if !self.consumer.before_fetch()? {
            self.stopped = true;
        }
        Ok(!self.stopped)
    }
    fn push(
        &mut self,
        plan: &DecodePlan,
        row: &[u8],
        _: &Config,
        native: &mut impl NativeReader,
    ) -> Result<(), Error> {
        if self.stopped {
            return Ok(());
        }
        let mut record = std::mem::take(&mut self.record);
        record.clear();
        if self.format == JsonFormat::Array && self.summary.rows != 0 {
            record.push(b',');
        }
        self.writer.row(plan, row, &mut record, native)?;
        if self.format == JsonFormat::Ndjson {
            record.push(b'\n');
            if self.output.data.len().saturating_add(record.len()) > self.limit {
                self.flush()?;
            }
            if !self.stopped {
                self.output.data.extend_from_slice(&record);
                self.output.rows += 1;
                // A single oversized NDJSON record is indivisible, like a row batch.
                if self.output.data.len() >= self.limit {
                    self.flush()?;
                }
            }
        } else {
            self.append_array(&record)?;
            if !self.stopped {
                self.output.rows += 1;
            }
        }
        if !self.stopped {
            self.summary.rows += 1;
        }
        self.record = record;
        Ok(())
    }
    fn after_fetch(&mut self) -> Result<bool, Error> {
        self.flush()?;
        Ok(!self.stopped)
    }
}

impl ExecutionConnection {
    pub fn stream_json_with(
        &mut self,
        sql: &str,
        params: &[Value],
        batch_rows: u32,
        json: (JsonOptions, JsonFormat),
        options: &QueryOptions<'_>,
        consumer: &mut impl BatchConsumer<JsonOutput>,
    ) -> Result<StreamSummary, Error> {
        if batch_rows == 0 {
            return Err(Error::config("batch_rows", "must be positive"));
        }
        let (mut summary, stopped, pending_rows) =
            self.operation(sql, options, |connection, entry, deadline| {
                let writer = JsonWriter::new(&entry.plan, json.0)?;
                let byte_rows = (connection.config.batch_bytes
                    / entry.plan.row_capacity_bytes()?.max(1) as u64)
                    .clamp(1, u32::MAX as u64) as u32;
                let mut sink = Chunks {
                    writer,
                    record: Vec::new(),
                    output: JsonOutput {
                        data: if json.1 == JsonFormat::Array {
                            vec![b'[']
                        } else {
                            Vec::new()
                        },
                        rows: 0,
                    },
                    limit: connection.config.batch_bytes as usize,
                    format: json.1,
                    consumer,
                    summary: StreamSummary::default(),
                    stopped: false,
                };
                connection.pump(
                    entry,
                    params,
                    options,
                    deadline,
                    batch_rows.min(byte_rows),
                    &mut sink,
                )?;
                Ok((sink.summary, sink.stopped, sink.output.rows))
            })?;
        // Only a successful cursor/transaction cleanup may complete a JSON array.
        // Waiting for this final chunk still observes demand and cancellation.
        if !stopped && json.1 == JsonFormat::Array && consumer.before_fetch()? {
            options.cancellation.check()?;
            check_deadline(options.deadline)?;
            consumer.batch(JsonOutput {
                data: vec![b']'],
                rows: pending_rows,
            })?;
            summary.bytes += 1;
            summary.batches += 1;
        }
        Ok(summary)
    }
}
