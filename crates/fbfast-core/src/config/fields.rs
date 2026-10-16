use serde::Serialize;
use std::{fmt, sync::Arc};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Secret(Arc<Zeroizing<String>>);

impl Secret {
    pub(crate) fn new(value: String) -> Self {
        Self(Arc::new(Zeroizing::new(value)))
    }

    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

// Serialization is diagnostic by default; binding explicitly handles password access.
impl Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("<redacted>")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Secret,
    pub role: Option<String>,
    pub read_only: bool,
    pub charset: String,
    pub session_time_zone: String,
    pub client_library: Option<String>,
    pub wire_crypt: String,
    pub wire_compression: bool,
    pub app_name: String,
    pub pool_size: u32,
    pub min_idle: u32,
    pub max_waiters: u32,
    pub acquire_timeout: f64,
    pub connect_timeout: f64,
    pub idle_timeout: f64,
    pub max_lifetime: f64,
    pub health_check_after: f64,
    pub close_timeout: f64,
    pub fetch_size: u32,
    pub batch_bytes: u64,
    pub prefetch_batches: u32,
    pub max_result_bytes: Option<u64>,
    pub max_result_rows: Option<u64>,
    pub max_blob_bytes: u64,
    pub query_timeout: Option<f64>,
    pub lock_timeout: u32,
    pub stream_idle_timeout: Option<f64>,
    pub statement_cache_size: u32,
    pub statement_cache_bytes: u64,
    pub trim_char: bool,
    pub fallback_charset: Option<String>,
}
