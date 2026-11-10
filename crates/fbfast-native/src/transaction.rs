use crate::{ffi, Connection, NativeError};

impl Connection {
    pub fn begin(
        &mut self,
        snapshot: bool,
        lock_seconds: u32,
        snapshot_number: Option<u64>,
    ) -> Result<(), NativeError> {
        self.check()?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: live exclusively owned connection and bounded scalar TPB values.
        unsafe {
            ffi::fbfast_begin(
                self.handle.as_ptr(),
                u32::from(snapshot),
                lock_seconds,
                snapshot_number.unwrap_or(0),
                &mut status,
            );
        }
        status.check("begin")
    }

    /// Rolls back, resets the session and starts a transaction with the same
    /// parameters in one round trip (`ALTER SESSION RESET` inside it).
    /// Returns whether the client kept the same transaction object.
    pub fn reset_restart(&mut self) -> Result<bool, NativeError> {
        self.check()?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: live exclusively owned connection and writable out status.
        let same = unsafe { ffi::fbfast_reset_restart(self.handle.as_ptr(), &mut status) };
        status.check("reset")?;
        Ok(same != 0)
    }

    pub fn rollback(&mut self) -> Result<(), NativeError> {
        self.call("rollback", ffi::fbfast_rollback)
    }
}
