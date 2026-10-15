use super::control::{deadline, Cancellation, QueryOptions};
use super::{cursor::check_deadline, ExecutionConnection};
use crate::{
    decode::{bind, DecodePlan, Value},
    error::Error,
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
            connection.pump(
                entry,
                params,
                options,
                deadline,
                if options.first {
                    1
                } else {
                    connection.config.fetch_size
                },
                &mut result,
            )?;
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

    pub(super) fn operation<T>(
        &mut self,
        sql: &str,
        options: &QueryOptions<'_>,
        action: impl FnOnce(&mut Self, &Entry, Option<Instant>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let deadline = self.operation_deadline(options.deadline);
        let validation = self
            .check()
            .and_then(|_| validate_read_query(sql))
            .and_then(|_| options.cancellation.check())
            .and_then(|_| check_deadline(deadline));
        if let Err(error) = validation {
            return if self.snapshot.is_some() {
                self.finish_operation(Err(error), None, options)
            } else {
                Err(error)
            };
        }
        if self.snapshot.is_none() {
            if let Err(error) = self.native.begin(false, self.config.lock_timeout, None) {
                self.healthy = false;
                return Err(error.into());
            }
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
        self.finish_operation(result, entry, options)
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
