//! One binding path for independent pool queries and a reserved snapshot owner.
use fbfast_core::{
    decode::Value,
    error::{Error, ErrorKind},
    pool::{Pending, Pool, Session, Stats, Stream},
};
use std::time::Duration;

#[derive(Clone)]
pub enum Executor {
    Pool(Pool),
    Session(Session),
}
impl Executor {
    pub fn stream_arrow(
        &self,
        sql: String,
        params: Vec<Value>,
        batch_rows: Option<u32>,
        timeout: Option<f64>,
    ) -> Result<Stream, Error> {
        match self {
            Self::Pool(pool) => pool.stream_arrow(sql, params, batch_rows, timeout),
            Self::Session(session) => session.stream_arrow(sql, params, batch_rows, timeout),
        }
    }
    pub fn submit_arrow(
        &self,
        sql: String,
        params: Vec<Value>,
        mode: fbfast_core::sinks::arrow::ArrowMode,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        match self {
            Self::Pool(pool) => pool.submit_arrow(sql, params, mode, timeout),
            Self::Session(session) => session.submit_arrow(sql, params, mode, timeout),
        }
    }
    pub fn submit_json(
        &self,
        sql: String,
        params: Vec<Value>,
        json: fbfast_core::sinks::json::JsonOptions,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        match self {
            Self::Pool(pool) => pool.submit_json(sql, params, json, timeout),
            Self::Session(session) => session.submit_json(sql, params, json, timeout),
        }
    }
    pub fn stream_json(
        &self,
        sql: String,
        params: Vec<Value>,
        batch_rows: Option<u32>,
        json: (
            fbfast_core::sinks::json::JsonOptions,
            fbfast_core::sinks::json::JsonFormat,
        ),
        timeout: Option<f64>,
    ) -> Result<Stream, Error> {
        match self {
            Self::Pool(pool) => pool.stream_json(sql, params, batch_rows, json, timeout),
            Self::Session(session) => session.stream_json(sql, params, batch_rows, json, timeout),
        }
    }
    pub fn snapshot(&self, timeout: Option<f64>) -> Result<Self, Error> {
        match self {
            Self::Pool(pool) => pool.snapshot(timeout).map(Self::Session),
            Self::Session(session) => {
                session.stats()?;
                Err(unsupported())
            }
        }
    }
    pub fn submit_rows(
        &self,
        sql: String,
        params: Vec<Value>,
        first: bool,
        unique_names: bool,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        match self {
            Self::Pool(pool) => pool.submit_rows(sql, params, first, unique_names, timeout),
            Self::Session(session) => {
                session.submit_rows(sql, params, first, unique_names, timeout)
            }
        }
    }
    pub fn submit_explain(
        &self,
        sql: String,
        params: Vec<Value>,
        timeout: Option<f64>,
    ) -> Result<Pending, Error> {
        match self {
            Self::Pool(pool) => pool.submit_explain(sql, params, timeout),
            Self::Session(session) => session.submit_explain(sql, params, timeout),
        }
    }
    pub fn stream(
        &self,
        sql: String,
        params: Vec<Value>,
        batch_rows: Option<u32>,
        unique_names: bool,
        timeout: Option<f64>,
    ) -> Result<Stream, Error> {
        match self {
            Self::Pool(pool) => pool.stream(sql, params, batch_rows, unique_names, timeout),
            Self::Session(session) => {
                session.stream(sql, params, batch_rows, unique_names, timeout)
            }
        }
    }
    pub fn warmup(&self) -> Result<Vec<Pending>, Error> {
        match self {
            Self::Pool(pool) => pool.warmup(),
            Self::Session(session) => session.open().map(|p| vec![p]),
        }
    }
    pub fn close(&self, duration: Duration) -> Result<(), Error> {
        match self {
            Self::Pool(pool) => pool.close(duration),
            Self::Session(session) => session.close(duration),
        }
    }
    pub fn begin_close(&self) -> Result<(), Error> {
        match self {
            Self::Pool(pool) => pool.begin_close(),
            Self::Session(session) => session.begin_close(),
        }
    }
    pub fn stats(&self) -> Result<Stats, Error> {
        match self {
            Self::Pool(pool) => pool.stats(),
            Self::Session(session) => session.stats(),
        }
    }
    pub fn clear_statement_cache(&self, timeout: Option<f64>) -> Result<Pending, Error> {
        match self {
            Self::Pool(pool) => pool.clear_statement_cache(timeout),
            Self::Session(session) => {
                session.stats()?;
                Err(unsupported())
            }
        }
    }
}
fn unsupported() -> Error {
    Error::new(
        ErrorKind::UnsupportedFeature,
        "snapshot",
        "Nested snapshots and cache invalidation are pool-only operations",
    )
}
