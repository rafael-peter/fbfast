use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseErrorKind {
    Other,
    Cancelled,
    StatementTimeout,
    NothingToCancel,
}

#[derive(Debug, Error, Clone)]
pub enum NativeError {
    #[error("client library {path}: {reason}")]
    Library { path: String, reason: String },
    #[error("client already initialized from {loaded}; cannot load {requested}")]
    ClientConflict { loaded: String, requested: String },
    #[error("unsupported client version {0}; required Firebird 5.0.3 or newer in 5.0.x")]
    UnsupportedVersion(String),
    #[error("Firebird runtime inherited across fork; initialize in a fresh spawned process")]
    ForkSafety,
    #[error("Firebird client initialization lock is poisoned")]
    InitializationPoisoned,
    #[error("{phase}: {message} (SQLSTATE {sqlstate})")]
    Database {
        kind: DatabaseErrorKind,
        phase: &'static str,
        message: String,
        sqlstate: String,
        codes: Vec<i64>,
    },
    #[error("{phase}: {message}")]
    Bridge {
        phase: &'static str,
        message: String,
    },
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("native cleanup failed: {0:?}")]
    Cleanup(Vec<NativeError>),
}

impl crate::ffi::RawStatus {
    pub fn check(&self, phase: &'static str) -> Result<(), NativeError> {
        if self.category == 0 {
            return Ok(());
        }
        let message = text(&self.message);
        if self.category == 1 {
            Err(NativeError::Database {
                kind: match self.kind {
                    1 => DatabaseErrorKind::Cancelled,
                    2 => DatabaseErrorKind::StatementTimeout,
                    3 => DatabaseErrorKind::NothingToCancel,
                    _ => DatabaseErrorKind::Other,
                },
                phase,
                message,
                sqlstate: text(&self.sqlstate),
                codes: self.codes[..(self.code_count as usize).min(self.codes.len())].to_vec(),
            })
        } else {
            Err(NativeError::Bridge { phase, message })
        }
    }
}

fn text(value: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = value
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(feature = "fuzzing")]
pub fn fuzz_status(data: &[u8]) {
    let mut raw = crate::ffi::RawStatus {
        category: data.first().copied().unwrap_or_default() as i32 % 3,
        kind: data.get(1).copied().unwrap_or_default() as u32,
        code_count: data.get(2).copied().unwrap_or_default() as u32,
        ..Default::default()
    };
    for (target, source) in raw.message.iter_mut().zip(data.iter().skip(3).copied()) {
        *target = source as std::ffi::c_char;
    }
    for (target, source) in raw.sqlstate.iter_mut().zip(data.iter().skip(11).copied()) {
        *target = source as std::ffi::c_char;
    }
    let _ = raw.check("fuzz_status");
}
