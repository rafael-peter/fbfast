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
        if let Err(error) = self.native.close_blob() {
            cleanup.push(error.into());
        }
        if let Err(error) = self.native.close_cursor() {
            cleanup.push(error.into());
        }
        // Successful operations inside an explicit snapshot keep its transaction.
        // Any error terminates that scope; an uncertain attachment is discarded.
        let abort = result.is_err() || !cleanup.is_empty() || !self.healthy;
        if self.defer_cleanup && self.snapshot.is_none() && !abort {
            // The result is complete and the cursor closed: the caller can have
            // it now. Rollback, reset and caching keep their order, later.
            self.deferred = Some(entry);
            self.mark("cleanup");
            return result;
        }
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
        self.mark("cleanup");
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

    /// Completes a deferred cleanup in the original order: rollback, session
    /// reset, then the statement goes back to the cache. Any failure marks the
    /// attachment unhealthy so its owner discards it; the result was valid.
    /// Returns whether a fresh transaction was left ready for the next query.
    pub fn finish_deferred_cleanup(&mut self) -> Result<bool, Error> {
        let Some(entry) = self.deferred.take() else {
            return Ok(false);
        };
        // Reuse needs every row read: an interrupted query may leave inline
        // BLOBs in the client cache, which a restarted transaction would keep.
        let reuse = self.config.ready_transaction_timeout.is_some()
            && self.drained
            && std::mem::take(&mut self.restartable);
        let finished = if reuse {
            self.native.reset_restart().map(|_| {
                self.ready = Some(std::time::Instant::now());
            })
        } else {
            self.native.rollback().and_then(|_| self.native.reset())
        };
        let finished = finished.map_err(Error::from).and_then(|_| match entry {
            Some(entry) => self.cache.put(
                entry,
                &mut self.native,
                self.config.statement_cache_size,
                self.config.statement_cache_bytes,
            ),
            None => Ok(()),
        });
        if finished.is_err() {
            self.healthy = false;
        }
        finished.map(|_| reuse)
    }
}
