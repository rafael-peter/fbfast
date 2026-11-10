//! One request type for every materialized query and every stream, shared by
//! pools, snapshots and language bindings.
use crate::{
    decode::Value,
    error::Error,
    sinks::{
        arrow::ArrowMode,
        json::{JsonFormat, JsonOptions},
    },
};

/// The shape of a materialized result.
#[derive(Debug, Clone, Copy)]
pub enum Request {
    /// Decoded rows; `first` stops after one row, `unique_names` rejects
    /// duplicate column names (needed for dictionaries and models).
    Rows {
        first: bool,
        unique_names: bool,
    },
    Json(JsonOptions),
    Arrow(ArrowMode),
    /// Execution plan; binds parameters but never executes.
    Explain,
}

/// The shape of each streamed batch.
#[derive(Debug, Clone, Copy)]
pub enum StreamFormat {
    Rows { unique_names: bool },
    Json(JsonOptions, JsonFormat),
    Arrow,
}

pub struct Query {
    pub sql: String,
    pub params: Vec<Value>,
    pub timeout: Option<f64>,
    pub request: Request,
}

pub struct StreamQuery {
    pub sql: String,
    pub params: Vec<Value>,
    pub timeout: Option<f64>,
    /// `None` uses `fetch_size`; `batch_bytes` also bounds every batch.
    pub batch_rows: Option<u32>,
    pub format: StreamFormat,
}

impl Query {
    pub fn new(sql: String, params: Vec<Value>, timeout: Option<f64>, request: Request) -> Self {
        Self {
            sql,
            params,
            timeout,
            request,
        }
    }

    pub fn rows(
        sql: String,
        params: Vec<Value>,
        first: bool,
        unique_names: bool,
        timeout: Option<f64>,
    ) -> Self {
        Self::new(
            sql,
            params,
            timeout,
            Request::Rows {
                first,
                unique_names,
            },
        )
    }

    /// The only SQL tokenization of a pooled query; workers trust this result.
    pub(super) fn validate(&self) -> Result<(), Error> {
        crate::statements::validate_read_query(&self.sql)?;
        crate::execution::control::deadline(self.timeout).map(drop)
    }
}

impl StreamQuery {
    pub(super) fn validate(&self, fetch_size: u32) -> Result<u32, Error> {
        crate::statements::validate_read_query(&self.sql)?;
        crate::execution::control::deadline(self.timeout)?;
        let batch_rows = self.batch_rows.unwrap_or(fetch_size);
        if batch_rows == 0 {
            return Err(Error::config("batch_rows", "must be positive"));
        }
        Ok(batch_rows)
    }
}
