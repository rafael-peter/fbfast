use super::{control::QueryOptions, cursor::RowSink, ExecutionConnection};
use crate::{
    config::Config,
    decode::{DecodePlan, NativeReader, Value},
    error::{Error, ErrorKind},
    sinks::json::{JsonOptions, JsonOutput, JsonWriter},
    sinks::limits::check_materialized_bytes,
};

struct Materialized {
    writer: JsonWriter,
    output: JsonOutput,
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
            .is_some_and(|limit| self.output.rows as u64 >= limit)
        {
            return Err(Error::new(
                ErrorKind::ResultLimit,
                "materialize",
                "Materialized result exceeds max_result_rows",
            ));
        }
        if self.output.rows != 0 {
            self.output.data.push(b',');
        }
        self.writer.row(plan, row, &mut self.output.data, native)?;
        check_materialized_bytes(self.output.data.len(), config.max_result_bytes)?;
        self.output.rows += 1;
        Ok(())
    }
}
impl ExecutionConnection {
    pub fn json_with(
        &mut self,
        sql: &str,
        params: &[Value],
        json: JsonOptions,
        options: &QueryOptions<'_>,
    ) -> Result<JsonOutput, Error> {
        self.operation(sql, options, |connection, entry, deadline| {
            let mut sink = Materialized {
                writer: JsonWriter::new(&entry.plan, json)?,
                output: JsonOutput {
                    data: vec![b'['],
                    rows: 0,
                },
            };
            connection.pump(
                entry,
                params,
                options,
                deadline,
                connection.config.fetch_size,
                &mut sink,
            )?;
            sink.output.data.push(b']');
            check_materialized_bytes(sink.output.data.len(), connection.config.max_result_bytes)?;
            Ok(sink.output)
        })
    }
}
