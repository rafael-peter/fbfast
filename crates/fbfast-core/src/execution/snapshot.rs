use super::{
    control::{deadline, QueryOptions},
    cursor::check_deadline,
    ExecutionConnection,
};
use crate::error::{Error, ErrorKind};
use std::time::Instant;

pub(super) struct Snapshot {
    pub deadline: Option<Instant>,
}

impl ExecutionConnection {
    /// Worker-only scope: the scheduler must reserve this owner until finish.
    pub fn begin_snapshot(&mut self, timeout: Option<f64>) -> Result<(), Error> {
        self.check()?;
        if self.snapshot.is_some() {
            return Err(Error::new(
                ErrorKind::Query,
                "snapshot",
                "A snapshot is already active",
            ));
        }
        let deadline = deadline(timeout)?;
        check_deadline(deadline)?;
        if let Err(error) = self.native.begin(true, self.config.lock_timeout, None) {
            self.healthy = false;
            return Err(error.into());
        }
        self.snapshot = Some(Snapshot { deadline });
        Ok(())
    }

    pub fn snapshot_active(&self) -> bool {
        self.snapshot.is_some()
    }

    /// Controller uses the same deadline, including time between operations.
    pub fn snapshot_deadline(&self) -> Option<Instant> {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.deadline)
    }

    pub fn finish_snapshot(&mut self, options: &QueryOptions<'_>) -> Result<(), Error> {
        let Some(snapshot) = self.snapshot.take() else {
            return Ok(());
        };
        let result = options
            .cancellation
            .check()
            .and_then(|_| check_deadline(snapshot.deadline));
        self.finish_operation(result, None, options)
    }

    pub(super) fn operation_deadline(&self, requested: Option<Instant>) -> Option<Instant> {
        match (requested, self.snapshot_deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
