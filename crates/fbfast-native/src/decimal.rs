use crate::{ffi, Connection, NativeError};
use std::ffi::CString;

fn size(kind: u32) -> Result<usize, NativeError> {
    match kind {
        32760 => Ok(8),
        32762 => Ok(16),
        _ => Err(NativeError::InvalidInput("Unsupported DECFLOAT type")),
    }
}

impl Connection {
    pub fn decimal_text(&mut self, kind: u32, data: &[u8]) -> Result<String, NativeError> {
        self.check()?;
        if data.len() != size(kind)? {
            return Err(NativeError::InvalidInput("Invalid DECFLOAT buffer length"));
        }
        // SDK IDecFloat34::STRING_SIZE is 43, including the terminator.
        let mut text = [0u8; 64];
        let mut status = ffi::RawStatus::default();
        // SAFETY: exact input width; bridge copies to aligned SDK storage and
        // receives the live output buffer's actual capacity.
        unsafe {
            ffi::fbfast_decimal_text(self.handle.as_ptr(), kind, data.as_ptr(),
                text.as_mut_ptr().cast(), text.len() as u32, &mut status);
        }
        status.check("decode_decfloat")?;
        let length = text.iter().position(|b| *b == 0)
            .ok_or(NativeError::InvalidInput("Unterminated DECFLOAT text"))?;
        String::from_utf8(text[..length].to_vec())
            .map_err(|_| NativeError::InvalidInput("Invalid DECFLOAT text encoding"))
    }

    /// Codec only. The core must compare the decoded result to reject rounding.
    pub fn decimal_bytes(&mut self, kind: u32, text: &str) -> Result<Vec<u8>, NativeError> {
        self.check()?;
        let mut data = vec![0u8; size(kind)?];
        let text = CString::new(text)
            .map_err(|_| NativeError::InvalidInput("Invalid DECFLOAT text"))?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: terminated input, exact writable output width; all data live
        // through the synchronous official utility call.
        unsafe { ffi::fbfast_decimal_bytes(self.handle.as_ptr(), kind, text.as_ptr(), data.as_mut_ptr(), &mut status); }
        status.check("bind_decfloat")?;
        Ok(data)
    }
}
