use crate::{ffi, Connection, NativeError, StatementId};

/// Aligned owned wire messages. BLOB IDs must be consumed in their transaction;
/// this storage only owns the ID bytes, not the referenced BLOB contents.
pub struct RawBatch {
    data: Vec<u64>,
    stride: usize,
    length: usize,
    count: usize,
    pub eof: bool,
}

impl RawBatch {
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn row(&self, index: usize) -> Option<&[u8]> {
        if index >= self.count {
            return None;
        }
        // SAFETY: data is initialized u64 storage for count*stride bytes; row
        // bounds checked above, message length <= stride, borrow cannot outlive data.
        Some(unsafe {
            std::slice::from_raw_parts(
                self.data.as_ptr().cast::<u8>().add(index * self.stride),
                self.length,
            )
        })
    }
}

impl Connection {
    pub fn open_cursor(
        &mut self,
        statement: StatementId,
        input: &[u8],
        timeout_ms: u32,
    ) -> Result<(), NativeError> {
        let id = self.validate_statement(statement)?;
        let length = u32::try_from(input.len())
            .map_err(|_| NativeError::InvalidInput("Input message too large"))?;
        let output = self.metadata(statement, false)?;
        let stride = output
            .message_length
            .checked_add(7)
            .map(|n| n / 8 * 8)
            .ok_or(NativeError::InvalidInput("Output message too large"))?;
        let mut aligned = vec![0u64; input.len().div_ceil(8)];
        // SAFETY: aligned storage fits the complete input byte slice, no overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(input.as_ptr(), aligned.as_mut_ptr().cast(), input.len());
        }
        let mut status = ffi::RawStatus::default();
        // SAFETY: registered statement, input remains aligned/live until synchronous execute returns.
        unsafe {
            ffi::fbfast_open_cursor(
                self.handle.as_ptr(),
                id,
                aligned.as_ptr().cast(),
                length,
                timeout_ms,
                &mut status,
            );
        }
        status.check("execute")?;
        self.stride = stride;
        self.message_length = output.message_length;
        Ok(())
    }

    pub fn fetch(&mut self, max_rows: u32, max_bytes: usize) -> Result<RawBatch, NativeError> {
        self.check()?;
        if self.stride == 0 || max_rows == 0 || max_bytes == 0 {
            return Err(NativeError::InvalidInput(
                "Fetch requires an open cursor and positive batch limits",
            ));
        }
        let stride = self.stride as usize;
        let rows = (max_bytes / stride).max(1).min(max_rows as usize);
        let bytes = rows
            .checked_mul(stride)
            .ok_or(NativeError::InvalidInput("Batch size overflow"))?;
        let mut data = Vec::new();
        data.try_reserve_exact(bytes / 8)
            .map_err(|_| NativeError::InvalidInput("Batch allocation failed"))?;
        data.resize(bytes / 8, 0u64);
        let mut status = ffi::RawStatus::default();
        let mut eof = 0;
        // SAFETY: buffer fits rows*stride, is 8-byte aligned and exclusively writable.
        let count = unsafe {
            ffi::fbfast_fetch(
                self.handle.as_ptr(),
                data.as_mut_ptr().cast(),
                self.stride,
                rows as u32,
                &mut eof,
                &mut status,
            )
        };
        status.check("fetch")?;
        Ok(RawBatch {
            data,
            stride,
            length: self.message_length as usize,
            count: count as usize,
            eof: eof != 0,
        })
    }

    pub fn close_cursor(&mut self) -> Result<(), NativeError> {
        self.call("close_cursor", ffi::fbfast_close_cursor)?;
        self.stride = 0;
        self.message_length = 0;
        Ok(())
    }
}
