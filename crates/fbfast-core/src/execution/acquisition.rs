use crate::{
    config::Config,
    error::{Error, ErrorKind},
};
use std::time::{Duration, Instant};

pub(super) struct ReadinessBudget {
    pub end: Instant,
    phase: &'static str,
}

impl ReadinessBudget {
    pub fn new(config: &Config, acquire_end: Option<Instant>) -> Result<Self, Error> {
        let duration = Duration::try_from_secs_f64(config.connect_timeout)
            .map_err(|_| Error::config("connect_timeout", "invalid duration"))?;
        let connect_end = Instant::now()
            .checked_add(duration)
            .ok_or_else(|| Error::config("connect_timeout", "duration overflow"))?;
        let budget = match acquire_end {
            Some(end) if end <= connect_end => Self {
                end,
                phase: "acquire",
            },
            _ => Self {
                end: connect_end,
                phase: "attach",
            },
        };
        budget.check()?;
        Ok(budget)
    }
    pub fn check(&self) -> Result<(), Error> {
        if Instant::now() >= self.end {
            Err(Error::new(
                ErrorKind::PoolTimeout,
                self.phase,
                "Connection readiness deadline expired; native cleanup may still be pending",
            ))
        } else {
            Ok(())
        }
    }
    pub fn native_seconds(&self) -> Result<u32, Error> {
        self.check()?;
        // DPB accepts whole seconds. Caller-side acquisition remains finer-grained;
        // a native call that outlives it keeps its slot until explicit cleanup.
        Ok(self
            .end
            .saturating_duration_since(Instant::now())
            .as_secs_f64()
            .ceil()
            .max(1.0) as u32)
    }
    pub fn classify(&self, mut error: Error) -> Error {
        if self.check().is_err() {
            error.kind = ErrorKind::PoolTimeout;
            error.phase = self.phase;
        }
        error
    }
}
