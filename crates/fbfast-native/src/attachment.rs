use crate::{ffi, Client, NativeError, StatementId};
use std::{
    collections::BTreeSet,
    ffi::CString,
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// Only transport parameters belong here; defaults and application validation live in core.
pub struct AttachOptions<'a> {
    pub database: &'a str,
    pub user: &'a str,
    pub password: &'a str,
    pub role: &'a str,
    pub charset: &'a str,
    pub time_zone: &'a str,
    pub app_name: &'a str,
    pub wire_crypt: &'a str,
    pub wire_compression: bool,
    pub connect_seconds: u32,
}

/// Thread-affine handle owner. Construct and close it inside its owning worker.
/// Statements are opaque, attachment-scoped IDs, never C++ pointers in the core.
pub struct Connection {
    pub(crate) handle: NonNull<std::ffi::c_void>,
    pub(crate) owner: u64,
    pub(crate) statements: BTreeSet<u64>,
    pub(crate) stride: u32,
    pub(crate) message_length: u32,
    pub(crate) _client: Arc<Client>,
    pid: u32,
    closed: bool,
    _thread_affine: PhantomData<Rc<()>>,
}

impl Connection {
    pub fn attach(client: Arc<Client>, options: &AttachOptions<'_>) -> Result<Self, NativeError> {
        client.check_process()?;
        let owner = NEXT_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| NativeError::InvalidInput("Connection identifiers exhausted"))?;
        if !matches!(options.wire_crypt, "required" | "enabled" | "disabled") {
            return Err(NativeError::InvalidInput("Invalid wire crypt policy"));
        }
        let config = format!(
            "Providers=Remote\nWireCrypt={}\nWireCompression={}\n",
            options.wire_crypt,
            if options.wire_compression {
                "true"
            } else {
                "false"
            }
        );
        let values = [
            options.database,
            options.user,
            options.password,
            options.role,
            options.charset,
            options.time_zone,
            options.app_name,
            &config,
        ]
        .into_iter()
        .map(|s| {
            CString::new(s).map_err(|_| NativeError::InvalidInput("Attach option contains NUL"))
        })
        .collect::<Result<Vec<_>, _>>()?;
        let raw = ffi::RawAttach {
            database: values[0].as_ptr(),
            user: values[1].as_ptr(),
            password: values[2].as_ptr(),
            role: values[3].as_ptr(),
            charset: values[4].as_ptr(),
            time_zone: values[5].as_ptr(),
            app_name: values[6].as_ptr(),
            config: values[7].as_ptr(),
            connect_seconds: options.connect_seconds,
        };
        let mut status = ffi::RawStatus::default();
        // SAFETY: Client owns the symbols; all C strings and out status live through the call.
        let pointer =
            unsafe { ffi::fbfast_attach((client.master)(), client.sqlstate, &raw, &mut status) };
        status.check("attach")?;
        let handle = NonNull::new(pointer).ok_or(NativeError::InvalidInput(
            "Attach returned null without an error",
        ))?;
        Ok(Self {
            handle,
            owner,
            statements: BTreeSet::new(),
            stride: 0,
            message_length: 0,
            _client: client,
            pid: std::process::id(),
            closed: false,
            _thread_affine: PhantomData,
        })
    }

    pub(crate) fn check(&self) -> Result<(), NativeError> {
        if self.pid != std::process::id() {
            return Err(NativeError::ForkSafety);
        }
        if self.closed {
            return Err(NativeError::InvalidInput("Connection is closed"));
        }
        Ok(())
    }

    pub(crate) fn call(
        &mut self,
        phase: &'static str,
        function: unsafe extern "C" fn(*mut std::ffi::c_void, *mut ffi::RawStatus),
    ) -> Result<(), NativeError> {
        self.check()?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: live exclusively owned connection and writable out status.
        unsafe {
            function(self.handle.as_ptr(), &mut status);
        }
        status.check(phase)
    }

    pub fn reset(&mut self) -> Result<(), NativeError> {
        self.call("reset", ffi::fbfast_reset)
    }
    pub fn ping(&mut self) -> Result<(), NativeError> {
        self.call("ping", ffi::fbfast_ping)
    }

    pub fn close(&mut self) -> Result<(), NativeError> {
        if self.pid != std::process::id() {
            return Err(NativeError::ForkSafety);
        }
        if self.closed {
            return Ok(());
        }
        let mut failures = Vec::new();
        // SAFETY: prevents every late/shared cancel call before protocol cleanup.
        // Waiting occurs on the owning worker, never in the consumer's Drop.
        let raised = unsafe { ffi::fbfast_control_invalidate(self.handle.as_ptr()) };
        if raised != 0 {
            if let Err(error) = self.disable_cancel() {
                failures.push(error);
            }
        }
        if let Err(e) = self.close_blob() {
            failures.push(e);
        }
        if let Err(e) = self.close_cursor() {
            failures.push(e);
        }
        for id in self.statements.clone() {
            if let Err(e) = self.free_statement(StatementId {
                owner: self.owner,
                id,
            }) {
                failures.push(e);
            }
        }
        if let Err(e) = self.rollback() {
            failures.push(e);
        }
        if let Err(e) = self.call("detach", ffi::fbfast_disconnect) {
            failures.push(e);
        }
        self.closed = true;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(NativeError::Cleanup(failures))
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if self.pid == std::process::id() {
            // SAFETY: unique owner; no cursor/statement raw pointers escape. This
            // only releases refs; explicit wire cleanup runs in close/worker flow.
            unsafe {
                ffi::fbfast_release(self.handle.as_ptr());
            }
        }
        // An inherited handle is deliberately untouched in a fork child.
    }
}
