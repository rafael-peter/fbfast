use super::{control::Cancellation, cursor::check_deadline};
use crate::error::{Error, ErrorKind};
use fbfast_native::Connection;
use std::time::Instant;

pub(super) struct Blobs<'a> {
    pub native: &'a mut Connection,
    pub cancellation: &'a Cancellation,
    pub deadline: Option<Instant>,
    pub limit: u64,
}

impl Blobs<'_> {
    pub(super) fn read(&mut self, id: [u8; 8], charset: u32) -> Result<Vec<u8>, Error> {
        self.cancellation.check()?;
        check_deadline(self.deadline)?;
        self.native.open_blob(id, charset)?;
        self.consume()
    }
}

impl Blobs<'_> {
    fn consume(&mut self) -> Result<Vec<u8>, Error> {
        let mut output = Vec::new();
        let mut buffer = [0u8; 32768];
        loop {
            self.cancellation.check()?;
            check_deadline(self.deadline)?;
            // Read one extra byte at the boundary to distinguish exact fit/EOF
            // from overflow, without allocating an unbounded or truncated result.
            let capacity = (self.limit - output.len() as u64 + 1).min(buffer.len() as u64) as usize;
            let (count, eof) = self.native.read_blob(&mut buffer[..capacity])?;
            self.cancellation.check()?;
            check_deadline(self.deadline)?;
            if count as u64 > self.limit - output.len() as u64 {
                return Err(Error::new(
                    ErrorKind::ResultLimit,
                    "read_blob",
                    "BLOB exceeds max_blob_bytes",
                ));
            }
            output.try_reserve(count).map_err(|_| {
                Error::new(
                    ErrorKind::ResultLimit,
                    "read_blob",
                    "BLOB allocation failed",
                )
            })?;
            output.extend_from_slice(&buffer[..count]);
            if eof {
                self.native.close_blob()?;
                return Ok(output);
            }
        }
        // On failure the operation cleanup first disarms cancellation, then
        // closes this BLOB, closes the cursor and rolls back the transaction.
    }
}

#[cfg(test)]
#[path = "blob_tests.rs"]
mod tests;
