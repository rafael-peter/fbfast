use crate::{ffi, Connection, NativeError};

impl Connection {
    /// One BLOB at a time, pinned to this cursor/transaction. No raw handle escapes.
    pub fn open_blob(&mut self, id: [u8; 8], charset: u32) -> Result<(), NativeError> {
        self.check()?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: exclusive live connection; the bridge copies all 8 ID bytes.
        unsafe { ffi::fbfast_open_blob(self.handle.as_ptr(), id.as_ptr(), charset, &mut status) };
        status.check("open_blob")
    }

    /// Returns (bytes written, EOF). Empty segments are distinct from EOF.
    pub fn read_blob(&mut self, buffer: &mut [u8]) -> Result<(usize, bool), NativeError> {
        self.check()?;
        if buffer.is_empty() || buffer.len() > 65535 {
            return Err(NativeError::InvalidInput(
                "Invalid BLOB segment buffer length",
            ));
        }
        let mut status = ffi::RawStatus::default();
        let mut eof = 0;
        // SAFETY: mutable buffer fits capacity and lives throughout the call.
        let count = unsafe {
            ffi::fbfast_read_blob(
                self.handle.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut eof,
                &mut status,
            )
        };
        status.check("read_blob")?;
        Ok((count as usize, eof != 0))
    }

    pub fn close_blob(&mut self) -> Result<(), NativeError> {
        self.call("close_blob", ffi::fbfast_close_blob)
    }
}
