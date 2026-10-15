use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Configuration,
    UnsupportedFeature,
    UnsupportedStatement,
    ReadOnlyViolation,
    ClientLibrary,
    ForkSafety,
    Query,
    Authentication,
    RoleNotActive,
    UnsupportedServer,
    DataConversion,
    UnsupportedType,
    ResultLimit,
    Cleanup,
    QueryTimeout,
    PoolClosed,
    PoolBusy,
    PoolTimeout,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub phase: &'static str,
    pub sqlstate: Option<String>,
    pub native_codes: Vec<i64>,
    pub cleanup: Vec<Error>,
}

impl Error {
    pub fn new(kind: ErrorKind, phase: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            phase,
            message: message.into(),
            sqlstate: None,
            native_codes: Vec::new(),
            cleanup: Vec::new(),
        }
    }
    pub fn config(field: &str, reason: &str) -> Self {
        Self::new(
            ErrorKind::Configuration,
            "config",
            format!("{field}: {reason}"),
        )
    }

    pub fn unsupported(message: &str) -> Self {
        Self::new(ErrorKind::UnsupportedFeature, "config", message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<fbfast_native::NativeError> for Error {
    fn from(error: fbfast_native::NativeError) -> Self {
        use fbfast_native::NativeError;
        match error {
            NativeError::Database {
                kind: native_kind,
                phase,
                message,
                sqlstate,
                codes,
            } => {
                let kind = if native_kind == fbfast_native::DatabaseErrorKind::StatementTimeout {
                    ErrorKind::QueryTimeout
                } else if native_kind == fbfast_native::DatabaseErrorKind::Cancelled {
                    ErrorKind::Cancelled
                } else if sqlstate == "28000" {
                    ErrorKind::Authentication
                } else {
                    ErrorKind::Query
                };
                Self {
                    kind,
                    message,
                    phase,
                    sqlstate: Some(sqlstate),
                    native_codes: codes,
                    cleanup: Vec::new(),
                }
            }
            NativeError::Bridge { phase, message } => Self::new(ErrorKind::Query, phase, message),
            NativeError::ForkSafety => {
                Self::new(ErrorKind::ForkSafety, "lifecycle", error.to_string())
            }
            NativeError::InvalidInput(message) => {
                Self::new(ErrorKind::DataConversion, "native", message)
            }
            NativeError::Cleanup(errors) => {
                let mut result = Self::new(ErrorKind::Cleanup, "cleanup", "Native cleanup failed");
                result.cleanup = errors.into_iter().map(Self::from).collect();
                result
            }
            _ => Self::new(ErrorKind::ClientLibrary, "load_client", error.to_string()),
        }
    }
}
