use super::control::{deadline, Cancellation, QueryOptions};
use super::ExecutionConnection;
use crate::{
    decode::{bind, DecodePlan, Value},
    error::{Error, ErrorKind},
    sinks::rows::Rows,
    statements::{cache::Entry, validate_read_query},
};
use std::time::Instant;

impl ExecutionConnection {
    pub fn fetch_rows(
        &mut self,
        sql: &str,
        params: &[Value],
        first: bool,
        timeout: Option<f64>,
    ) -> Result<Rows, Error> {
        self.fetch_with(
            sql,
            params,
            &QueryOptions {
                first,
                unique_names: false,
                deadline: deadline(timeout)?,
                cancellation: &Cancellation::default(),
                cleaning: &|| Ok(false),
            },
        )
    }

    pub fn fetch_with(
        &mut self,
        sql: &str,
        params: &[Value],
        options: &QueryOptions<'_>,
    ) -> Result<Rows, Error> {
        self.operation(sql, options, |connection, entry, deadline| {
            let mut result = Rows::new(&entry.plan);
            if options.unique_names {
                result.require_unique_names()?;
            }
            let parameters = bind::encode(&entry.input, params)?;
            options.cancellation.check()?;
            check_deadline(deadline)?;
            let native_timeout = deadline
                .map(|end| {
                    end.saturating_duration_since(Instant::now())
                        .as_millis()
                        .clamp(1, u32::MAX as u128) as u32
                })
                .unwrap_or(0);
            connection
                .native
                .open_cursor(entry.id, &parameters, native_timeout)?;
            loop {
                options.cancellation.check()?;
                check_deadline(deadline)?;
                let batch = connection.native.fetch(
                    if options.first {
                        1
                    } else {
                        connection.config.fetch_size
                    },
                    connection.config.batch_bytes as usize,
                )?;
                for index in 0..batch.len() {
                    let row = batch.row(index).ok_or_else(|| {
                        Error::new(
                            ErrorKind::DataConversion,
                            "fetch",
                            "Native batch index invalid",
                        )
                    })?;
                    result.push(&entry.plan, row, &connection.config)?;
                    check_deadline(deadline)?;
                }
                if options.first || batch.eof {
                    break;
                }
            }
            Ok(result)
        })
    }

    pub fn explain(
        &mut self,
        sql: &str,
        params: &[Value],
        timeout: Option<f64>,
    ) -> Result<String, Error> {
        self.explain_with(
            sql,
            params,
            &QueryOptions {
                first: true,
                unique_names: false,
                deadline: deadline(timeout)?,
                cancellation: &Cancellation::default(),
                cleaning: &|| Ok(false),
            },
        )
    }

    pub fn explain_with(
        &mut self,
        sql: &str,
        params: &[Value],
        options: &QueryOptions<'_>,
    ) -> Result<String, Error> {
        self.operation(sql, options, |connection, entry, _| {
            // Validate exactly the query binding contract without executing it.
            let _parameters = bind::encode(&entry.input, params)?;
            connection.native.plan(entry.id).map_err(Error::from)
        })
    }

    fn operation<T>(
        &mut self,
        sql: &str,
        options: &QueryOptions<'_>,
        action: impl FnOnce(&mut Self, &Entry, Option<Instant>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.check()?;
        validate_read_query(sql)?;
        options.cancellation.check()?;
        let deadline = options.deadline;
        check_deadline(deadline)?;
        if let Err(error) = self.native.begin(false, self.config.lock_timeout, None) {
            self.healthy = false;
            return Err(error.into());
        }
        let (result, entry) = match self.prepare_entry(sql) {
            Ok(entry) => {
                let result = options
                    .cancellation
                    .check()
                    .and_then(|_| check_deadline(deadline))
                    .and_then(|_| action(self, &entry, deadline))
                    .and_then(|value| {
                        options.cancellation.check()?;
                        check_deadline(deadline)?;
                        Ok(value)
                    });
                (result, Some(entry))
            }
            Err(error) => (Err(error), None),
        };
        let mut cleanup = Vec::new();
        match (options.cleaning)() {
            Ok(true) => {
                self.healthy = false;
                if let Err(error) = self.native.disable_cancel() {
                    cleanup.push(error.into());
                }
            }
            Err(error) => cleanup.push(error),
            Ok(false) => (),
        }
        if let Err(e) = self.native.close_cursor() {
            cleanup.push(Error::from(e));
        }
        if let Err(e) = self.native.rollback() {
            cleanup.push(Error::from(e));
        }
        if cleanup.is_empty() {
            if let Err(e) = self.native.reset() {
                cleanup.push(Error::from(e));
            }
        }
        if cleanup.is_empty() {
            if let Some(entry) = entry {
                if let Err(error) = self.cache.put(
                    entry,
                    &mut self.native,
                    self.config.statement_cache_size,
                    self.config.statement_cache_bytes,
                ) {
                    cleanup.push(error);
                }
            }
        }
        if !cleanup.is_empty() {
            self.healthy = false;
        }
        match result {
            Err(mut error) => {
                if matches!(error.kind, ErrorKind::Cancelled | ErrorKind::QueryTimeout) {
                    if let Err(reason) = options.cancellation.check() {
                        error.kind = reason.kind;
                    }
                }
                error.cleanup.extend(cleanup);
                Err(error)
            }
            Ok(value) if cleanup.is_empty() => Ok(value),
            Ok(_) => {
                let mut error = Error::new(
                    ErrorKind::Cleanup,
                    "cleanup",
                    "Query succeeded but connection cleanup failed",
                );
                error.cleanup = cleanup;
                Err(error)
            }
        }
    }

    fn prepare_entry(&mut self, sql: &str) -> Result<Entry, Error> {
        if let Some(entry) = self.cache.take(sql) {
            return Ok(entry);
        }
        let id = self.native.prepare(sql)?;
        let described: Result<Entry, Error> = (|| {
            let input = self.native.metadata(id, true)?;
            let plan = DecodePlan::new(self.native.metadata(id, false)?, &self.config)?;
            Ok(Entry {
                sql: sql.into(),
                id,
                input,
                plan,
            })
        })();
        match described {
            Ok(entry) => Ok(entry),
            Err(mut error) => {
                if let Err(cleanup) = self.native.free_statement(id) {
                    error.cleanup.push(cleanup.into());
                }
                Err(error)
            }
        }
    }
}

fn check_deadline(deadline: Option<Instant>) -> Result<(), Error> {
    if deadline.is_some_and(|end| Instant::now() >= end) {
        return Err(Error::new(
            ErrorKind::QueryTimeout,
            "query",
            "Query deadline expired",
        ));
    }
    Ok(())
}
