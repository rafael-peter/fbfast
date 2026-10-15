use crate::{
    config::Config,
    error::{Error, ErrorKind},
    statements::cache::Cache,
};
use fbfast_native::{AttachOptions, Client, Connection};
use std::path::Path;

/// Worker-owned execution state. Pool/scheduler APIs must not expose this object
/// to Python or move its native handle to another thread.
pub struct ExecutionConnection {
    pub(crate) native: Connection,
    pub(crate) config: Config,
    pub(crate) cache: Cache,
    pub(crate) healthy: bool,
    control: Option<fbfast_native::CancelHandle>,
    session: Option<super::SessionInfo>,
}

impl ExecutionConnection {
    pub fn connect(config: Config, bundled_client: &Path) -> Result<Self, Error> {
        if !config.read_only {
            return Err(Error::unsupported("read_only=False is not supported"));
        }
        if !config.charset.eq_ignore_ascii_case("UTF8") {
            return Err(Error::unsupported(
                "The scalar prototype currently requires connection charset UTF8",
            ));
        }
        let host = if config.host.contains(':') && !config.host.starts_with('[') {
            format!("[{}]", config.host)
        } else {
            config.host.clone()
        };
        let database = format!("{host}/{}:{}", config.port, config.database);
        let path = config
            .client_library
            .as_deref()
            .map(Path::new)
            .unwrap_or(bundled_client);
        let client = Client::load(path)?;
        let native = Connection::attach(
            client,
            &AttachOptions {
                database: &database,
                user: &config.user,
                password: config.password.expose(),
                role: config.role.as_deref().unwrap_or(""),
                charset: &config.charset,
                time_zone: &config.session_time_zone,
                app_name: &config.app_name,
                wire_crypt: &config.wire_crypt,
                wire_compression: config.wire_compression,
                connect_seconds: config.connect_timeout.ceil() as u32,
            },
        )?;
        let mut connection = Self {
            native,
            config,
            cache: Cache::default(),
            healthy: true,
            control: None,
            session: None,
        };
        match connection.verify_session() {
            Ok(info) => connection.session = Some(info),
            Err(mut error) => {
                if let Err(cleanup) = connection.close() {
                    error.cleanup.push(cleanup);
                }
                return Err(error);
            }
        }
        Ok(connection)
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy
    }
    pub fn session_info(&self) -> &super::SessionInfo {
        self.session
            .as_ref()
            .expect("connection is published only after handshake")
    }
    pub(crate) fn begin_control(
        &mut self,
        generation: u64,
    ) -> Result<fbfast_native::CancelHandle, Error> {
        if self.control.is_none() {
            self.control = Some(self.native.cancel_handle()?);
        }
        self.native.begin_operation(generation)?;
        Ok(self.control.as_ref().expect("control initialized").clone())
    }
    pub(crate) fn poison(&mut self) {
        self.healthy = false;
    }
    pub fn cache_counts(&self) -> (u64, u64) {
        (self.cache.hits, self.cache.misses)
    }
    pub fn take_cache_counts(&mut self) -> (u64, u64) {
        (
            std::mem::take(&mut self.cache.hits),
            std::mem::take(&mut self.cache.misses),
        )
    }
    pub fn ping(&mut self) -> Result<(), Error> {
        if let Err(error) = self.native.ping() {
            self.healthy = false;
            return Err(error.into());
        }
        Ok(())
    }
    pub fn clear_statement_cache(&mut self) -> Result<(), Error> {
        let result = self.cache.clear(&mut self.native);
        if result.is_err() {
            self.healthy = false;
        }
        result
    }
    pub fn close(&mut self) -> Result<(), Error> {
        self.healthy = false;
        self.native.close().map_err(Error::from)
    }
    pub(crate) fn check(&self) -> Result<(), Error> {
        if !self.healthy {
            return Err(Error::new(
                ErrorKind::Query,
                "acquire",
                "Connection was closed or failed cleanup",
            ));
        }
        Ok(())
    }
}
