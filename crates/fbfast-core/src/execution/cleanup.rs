use super::{control::QueryOptions, ExecutionConnection};
use crate::{
    error::{Error, ErrorKind},
    statements::cache::Entry,
};

impl ExecutionConnection {
    pub(super) fn finish_operation<T>(
        &mut self,
        result: Result<T, Error>,
        entry: Option<Entry>,
        options: &QueryOptions<'_>,
    ) -> Result<T, Error> {
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
        if let Err(error) = self.native.close_cursor() {
            cleanup.push(error.into());
        }
        // Successful operations inside an explicit snapshot keep its transaction.
        // Any error terminates that scope; an uncertain attachment is discarded.
        let abort = result.is_err() || !cleanup.is_empty() || !self.healthy;
        if self.snapshot.is_none() || abort {
            if self.snapshot.take().is_some() && abort {
                self.healthy = false;
            }
            if let Err(error) = self.native.rollback() {
                cleanup.push(error.into());
            }
            if cleanup.is_empty() {
                if let Err(error) = self.native.reset() {
                    cleanup.push(error.into());
                }
            }
        }
        if cleanup.is_empty() {
            if let Some(entry) = entry {
                let result = if self.healthy {
                    self.cache.put(
                        entry,
                        &mut self.native,
                        self.config.statement_cache_size,
                        self.config.statement_cache_bytes,
                    )
                } else {
                    self.native.free_statement(entry.id).map_err(Error::from)
                };
                if let Err(error) = result {
                    cleanup.push(error);
                }
            }
        }
        if !cleanup.is_empty() {
            self.healthy = false;
            if self.snapshot.take().is_some() {
                if let Err(error) = self.native.rollback() {
                    cleanup.push(error.into());
                }
            }
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
                    "Operation succeeded but connection cleanup failed",
                );
                error.cleanup = cleanup;
                Err(error)
            }
        }
    }
}
