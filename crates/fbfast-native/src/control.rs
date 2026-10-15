//! The only handle allowed to cross worker threads. It can neither fetch nor detach.
use crate::{ffi, Client, Connection, NativeError};
use std::{ffi::c_void, ptr::NonNull, sync::Arc};

struct Shared {
    pointer: NonNull<c_void>,
    _client: Arc<Client>,
    pid: u32,
}

// SAFETY: C++ ControlRef owns shared state protected by its gate. Connection
// invalidates its borrowed owner under that same gate before cleanup/destruction.
// Raise/end never access mutable query handles, and every call owns fresh status.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

#[derive(Clone)]
pub struct CancelHandle(Arc<Shared>);

impl CancelHandle {
    fn check(&self) -> Result<(), NativeError> {
        if self.0.pid != std::process::id() {
            Err(NativeError::ForkSafety)
        } else {
            Ok(())
        }
    }

    pub fn request(&self, generation: u64) -> Result<bool, NativeError> {
        self.check()?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: shared reference lives through call; generation gate protects the owner.
        let raised =
            unsafe { ffi::fbfast_control_raise(self.0.pointer.as_ptr(), generation, &mut status) };
        // An operation may have just left the client. This documented race does
        // not mean a transport failure; still conservatively retire the attachment.
        if status.kind != 3 {
            status.check("cancel")?;
        }
        Ok(raised != 0)
    }

    /// Called on the owning worker before cleanup. Waits for any in-flight
    /// cancel call; caller-side timeouts never release this worker's slot early.
    pub fn finish(&self, generation: u64) -> Result<bool, NativeError> {
        self.check()?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: same shared lifetime and serialization as request().
        let raised =
            unsafe { ffi::fbfast_control_end(self.0.pointer.as_ptr(), generation, &mut status) };
        status.check("cancel_finish")?;
        Ok(raised != 0)
    }
}

impl Connection {
    pub fn cancel_handle(&mut self) -> Result<CancelHandle, NativeError> {
        self.check()?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: exclusive live connection; bridge allocates one owned ControlRef.
        let pointer = unsafe { ffi::fbfast_control_acquire(self.handle.as_ptr(), &mut status) };
        status.check("cancel_setup")?;
        let pointer =
            NonNull::new(pointer).ok_or(NativeError::InvalidInput("Null cancel control"))?;
        Ok(CancelHandle(Arc::new(Shared {
            pointer,
            _client: self._client.clone(),
            pid: std::process::id(),
        })))
    }

    pub fn begin_operation(&mut self, generation: u64) -> Result<(), NativeError> {
        self.check()?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: worker owns connection and begins one generation at a time.
        unsafe {
            ffi::fbfast_control_begin(self.handle.as_ptr(), generation, &mut status);
        }
        status.check("cancel_setup")
    }

    pub fn disable_cancel(&mut self) -> Result<(), NativeError> {
        self.call("cancel_disable", ffi::fbfast_cancel_disable)
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        if self.pid == std::process::id() {
            // SAFETY: final Arc owns exactly one ControlRef. This releases only
            // local shared state, never the attachment or a protocol resource.
            unsafe {
                ffi::fbfast_control_release(self.pointer.as_ptr());
            }
        }
    }
}
