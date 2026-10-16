use super::{input::Reader, Config, EnvFile, Inputs, Secret};
use crate::error::Error;
use serde_json::Value;
use std::{collections::BTreeMap, fs::File, path::Path};

impl Config {
    pub fn resolve(explicit: Inputs, prefix: &str, env_file: EnvFile) -> Result<Self, Error> {
        let mut environment = BTreeMap::new();
        for (key, value) in std::env::vars_os() {
            let Some(key) = key.to_str() else {
                continue;
            };
            if let Some(field) = key.strip_prefix(prefix) {
                if FIELDS.contains(&field.to_ascii_lowercase().as_str()) {
                    environment.insert(
                        key.into(),
                        value.into_string().map_err(|_| {
                            Error::config(field, "environment value is not Unicode")
                        })?,
                    );
                }
            }
        }
        Self::resolve_sources(explicit, prefix, env_file, &environment, Path::new("."))
    }

    /// Injectable sources permit tests without global environment mutation or network.
    pub fn resolve_sources(
        explicit: Inputs,
        prefix: &str,
        env_file: EnvFile,
        environment: &BTreeMap<String, String>,
        cwd: &Path,
    ) -> Result<Self, Error> {
        if prefix.is_empty()
            || !prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(Error::config(
                "prefix",
                "requires nonempty ASCII letters, digits or underscores",
            ));
        }
        let file = read_env(env_file, cwd)?;
        let mut values = Inputs::new();
        for source in [&file, environment] {
            for (key, value) in source {
                if let Some(field) = key.strip_prefix(prefix) {
                    // Ignore unrelated FB_* keys rather than treating any application
                    // environment variable as part of the library's API.
                    let field = field.to_ascii_lowercase();
                    if FIELDS.contains(&field.as_str()) {
                        values.insert(field, Value::String(value.clone()));
                    }
                }
            }
        }
        values.extend(explicit);
        Self::validated(Reader(values))
    }

    fn validated(mut r: Reader) -> Result<Self, Error> {
        let read_only = r.boolean("read_only", true)?;
        if !read_only {
            return Err(Error::unsupported("read_only=False is not supported"));
        }
        let wire_crypt = match r.0.remove("wire_crypt") {
            None | Some(Value::Bool(true)) => "required".to_owned(),
            Some(Value::Bool(false)) => "disabled".to_owned(),
            Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
                "required" | "enabled" | "disabled" => s.to_ascii_lowercase(),
                "true" | "1" | "yes" => "required".into(),
                "false" | "0" | "no" => "disabled".into(),
                _ => {
                    return Err(Error::config(
                        "wire_crypt",
                        "requires required/enabled/disabled",
                    ))
                }
            },
            _ => {
                return Err(Error::config(
                    "wire_crypt",
                    "requires required/enabled/disabled",
                ))
            }
        };
        let config = Self {
            host: r.text("host", Some("localhost"))?,
            port: r.integer("port", 3050, 1, u16::MAX as u64)? as u16,
            database: r.text("database", None)?,
            user: r.text("user", None)?,
            password: Secret::new(r.text("password", None)?),
            role: r.optional_text("role")?,
            read_only,
            charset: r.text("charset", Some("UTF8"))?,
            session_time_zone: r.text("session_time_zone", Some("UTC"))?,
            client_library: r.optional_text("client_library")?,
            wire_crypt,
            wire_compression: r.boolean("wire_compression", false)?,
            app_name: r.text("app_name", Some("fbfast"))?,
            pool_size: r.integer("pool_size", 8, 1, u32::MAX as u64)? as u32,
            min_idle: r.integer("min_idle", 2, 0, u32::MAX as u64)? as u32,
            max_waiters: r.integer("max_waiters", 256, 1, u32::MAX as u64)? as u32,
            acquire_timeout: r.seconds("acquire_timeout", 5.0)?,
            connect_timeout: r.seconds("connect_timeout", 5.0)?,
            idle_timeout: r.seconds("idle_timeout", 300.0)?,
            max_lifetime: r.seconds("max_lifetime", 1800.0)?,
            health_check_after: r.seconds("health_check_after", 30.0)?,
            close_timeout: r.seconds("close_timeout", 10.0)?,
            fetch_size: r.integer("fetch_size", 5000, 1, u32::MAX as u64)? as u32,
            batch_bytes: r.integer("batch_bytes", 4 * 1024 * 1024, 1, i64::MAX as u64)?,
            prefetch_batches: r.integer("prefetch_batches", 1, 0, u32::MAX as u64)? as u32,
            max_result_bytes: r.optional_integer("max_result_bytes", Some(512 * 1024 * 1024))?,
            max_result_rows: r.optional_integer("max_result_rows", None)?,
            max_blob_bytes: r.integer("max_blob_bytes", 64 * 1024 * 1024, 1, i64::MAX as u64)?,
            query_timeout: r.optional_seconds("query_timeout", 30.0)?,
            lock_timeout: r.integer("lock_timeout", 5, 0, i32::MAX as u64)? as u32,
            stream_idle_timeout: r.optional_seconds("stream_idle_timeout", 30.0)?,
            statement_cache_size: r.integer("statement_cache_size", 64, 0, u32::MAX as u64)? as u32,
            statement_cache_bytes: r.integer(
                "statement_cache_bytes",
                8 * 1024 * 1024,
                1,
                i64::MAX as u64,
            )?,
            trim_char: r.boolean("trim_char", false)?,
            fallback_charset: r.optional_text("fallback_charset")?,
        };
        r.finish()?;
        if config.min_idle > config.pool_size {
            return Err(Error::config("min_idle", "must not exceed pool_size"));
        }
        Ok(config)
    }
}

fn read_env(env_file: EnvFile, cwd: &Path) -> Result<BTreeMap<String, String>, Error> {
    let (path, optional) = match env_file {
        EnvFile::Disabled => return Ok(BTreeMap::new()),
        EnvFile::Default => (cwd.join(".env"), true),
        EnvFile::Path(path) => (cwd.join(path), false),
    };
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BTreeMap::new())
        }
        Err(_) => return Err(Error::config("env_file", "cannot read requested file")),
    };
    dotenvy::from_read_iter(file)
        .map(|result| {
            // dotenvy parse errors can embed a complete credential line: do not expose it.
            result.map_err(|_| Error::config("env_file", "invalid dotenv syntax"))
        })
        .collect()
}

const FIELDS: &[&str] = &[
    "host",
    "port",
    "database",
    "user",
    "password",
    "role",
    "read_only",
    "charset",
    "session_time_zone",
    "client_library",
    "wire_crypt",
    "wire_compression",
    "app_name",
    "pool_size",
    "min_idle",
    "max_waiters",
    "acquire_timeout",
    "connect_timeout",
    "idle_timeout",
    "max_lifetime",
    "health_check_after",
    "close_timeout",
    "fetch_size",
    "batch_bytes",
    "prefetch_batches",
    "max_result_bytes",
    "max_result_rows",
    "max_blob_bytes",
    "query_timeout",
    "lock_timeout",
    "stream_idle_timeout",
    "statement_cache_size",
    "statement_cache_bytes",
    "trim_char",
    "fallback_charset",
];
