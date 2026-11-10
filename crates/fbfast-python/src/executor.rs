//! One binding path for independent pool queries and a reserved snapshot owner.
use fbfast_core::{
    error::{Error, ErrorKind},
    pool::{Pending, Pool, Query, Session, Stats, Stream, StreamQuery},
};
use std::time::Duration;

#[derive(Clone)]
pub enum Executor {
    Pool(Pool),
    Session(Session),
}
impl Executor {
    pub fn submit(&self, query: Query) -> Result<Pending, Error> {
        match self {
            Self::Pool(pool) => pool.submit(query),
            Self::Session(session) => session.submit(query),
        }
    }
    pub fn open_stream(&self, query: StreamQuery) -> Result<Stream, Error> {
        match self {
            Self::Pool(pool) => pool.open_stream(query),
            Self::Session(session) => session.open_stream(query),
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
    pub fn session_info(&self) -> Result<Option<fbfast_core::execution::SessionInfo>, Error> {
        match self {
            Self::Pool(pool) => pool.session_info(),
            Self::Session(session) => {
                session.stats()?;
                Err(unsupported())
            }
        }
    }
    pub fn diagnostics(&self) -> Result<Vec<fbfast_core::execution::QueryTrace>, Error> {
        match self {
            Self::Pool(pool) => pool.diagnostics(),
            Self::Session(session) => {
                session.stats()?;
                Err(unsupported())
            }
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
        "Nested snapshots, cache invalidation and diagnostics are pool-only operations",
    )
}
