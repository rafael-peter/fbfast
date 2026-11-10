use crate::{
    config::Config,
    error::{Error, ErrorKind},
    statements::cache::Cache,
};
use fbfast_native::{AttachOptions, Client, Connection};
use std::{path::Path, time::Instant};

/// Worker-owned execution state. Pool/scheduler APIs must not expose this object
/// to Python or move its native handle to another thread.
pub struct ExecutionConnection {
    pub(crate) native: Connection,
    pub(crate) config: Config,
    pub(crate) cache: Cache,
    pub(crate) healthy: bool,
    control: Option<fbfast_native::CancelHandle>,
    session: Option<super::SessionInfo>,
    pub(super) snapshot: Option<super::snapshot::Snapshot>,
    pub(super) trace: Option<super::Trace>,
    /// Pool workers answer before rollback/reset; see `finish_deferred_cleanup`.
    pub(super) defer_cleanup: bool,
    /// `Some` while a successful operation still owes rollback, reset and the
    /// return of its statement to the cache (the inner value is that statement).
    pub(super) deferred: Option<Option<crate::statements::cache::Entry>>,
    /// Since when a pre-started transaction waits for the next operation.
    pub(super) ready: Option<Instant>,
    /// The last operation read every row, so no inline BLOB is left unread.
    pub(super) drained: bool,
    /// The open transaction is a pool transaction (READ COMMITTED READ
    /// CONSISTENCY). `ALTER SESSION RESET` restarts a transaction with the
    /// same parameters, so only these may become the next query's transaction;
    /// a snapshot's SNAPSHOT transaction never does.
    pub(super) restartable: bool,
}

impl ExecutionConnection {
    /// Test and tooling entry point; pool workers use `connect_before`.
    #[doc(hidden)]
    pub fn connect(config: Config, bundled_client: &Path) -> Result<Self, Error> {
        Self::connect_with_budget(config, bundled_client, None)
    }

    pub fn connect_before(
        config: Config,
        bundled_client: &Path,
        acquire_end: Instant,
    ) -> Result<Self, Error> {
        Self::connect_with_budget(config, bundled_client, Some(acquire_end))
    }

    fn connect_with_budget(
        config: Config,
        bundled_client: &Path,
        acquire_end: Option<Instant>,
    ) -> Result<Self, Error> {
        let budget = super::acquisition::ReadinessBudget::new(&config, acquire_end)?;
        if !config.read_only {
            return Err(Error::unsupported("read_only=False is not supported"));
        }
        if !config.charset.eq_ignore_ascii_case("UTF8") {
            return Err(Error::unsupported(
                "Only connection charset UTF8 is supported; text in other column \
                 charsets is converted by the server",
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
                connect_seconds: budget.native_seconds()?,
                max_inline_blob_size: config.max_inline_blob_size,
                blob_cache_size: config.blob_cache_size.map(|size| size as u32),
            },
        )
        .map_err(|error| budget.classify(error.into()))?;
        let mut connection = Self {
            native,
            config,
            cache: Cache::default(),
            healthy: true,
            control: None,
            session: None,
            snapshot: None,
            trace: None,
            defer_cleanup: false,
            deferred: None,
            ready: None,
            drained: false,
            restartable: false,
        };
        match budget
            .check()
            .and_then(|_| connection.verify_session(budget.end))
            .and_then(|info| {
                budget.check()?;
                Ok(info)
            }) {
            Ok(info) => connection.session = Some(info),
            Err(error) => {
                let mut error = budget.classify(error);
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
    /// Client-side wire counters of this attachment (no server round trip).
    pub fn wire_stats(&mut self) -> Result<Option<fbfast_native::WireStats>, Error> {
        self.native.wire_stats().map_err(Error::from)
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
        // Native close rolls back an open transaction and frees every statement.
        self.deferred = None;
        self.ready = None;
        self.native.close().map_err(Error::from)
    }

    /// Lets a successful operation return before its rollback and session reset.
    /// The owner must call `finish_deferred_cleanup` before the next operation.
    pub fn set_defer_cleanup(&mut self, defer: bool) {
        self.defer_cleanup = defer;
    }

    pub fn has_deferred_cleanup(&self) -> bool {
        self.deferred.is_some()
    }

    /// How long a pre-started transaction has been waiting, if there is one.
    pub fn ready_since(&self) -> Option<Instant> {
        self.ready
    }

    /// Rolls back a pre-started transaction nobody used (1 round trip).
    pub fn release_ready(&mut self) -> Result<(), Error> {
        if self.ready.take().is_none() {
            return Ok(());
        }
        let released = self.native.rollback().map_err(Error::from);
        if released.is_err() {
            self.healthy = false;
        }
        released
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
