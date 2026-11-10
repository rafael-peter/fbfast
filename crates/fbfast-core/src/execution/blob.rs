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

/// Firebird character set identifiers (RDB$CHARACTER_SETS).
const UTF8: u32 = 4;
const ISO8859_1: u32 = 21;

/// How a text BLOB reaches UTF-8 without a server-side BLOB filter.
///
/// The Firebird 5.0.3+ client only uses a BLOB that arrived inline with its row
/// when the BLOB is opened without a BPB; any transliteration BPB discards the
/// cached copy and costs extra round trips. UTF-8 text needs no conversion on a
/// UTF-8 connection and ISO8859_1 maps every byte to the code point of the same
/// value, so both are opened raw. Other charsets keep the official filter.
pub(super) fn local_text(charset: u32) -> Option<fn(Vec<u8>) -> Vec<u8>> {
    match charset {
        UTF8 => Some(|bytes| bytes),
        ISO8859_1 => Some(latin1_to_utf8),
        _ => None,
    }
}

pub(super) fn latin1_to_utf8(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.is_ascii() {
        return bytes;
    }
    let mut output = Vec::with_capacity(bytes.len() + bytes.len() / 2);
    for byte in bytes {
        if byte < 0x80 {
            output.push(byte);
        } else {
            output.extend_from_slice(&[0xc0 | (byte >> 6), 0x80 | (byte & 0x3f)]);
        }
    }
    output
}

impl Blobs<'_> {
    pub(super) fn read(&mut self, id: [u8; 8], charset: u32) -> Result<Vec<u8>, Error> {
        self.cancellation.check()?;
        check_deadline(self.deadline)?;
        let Some(convert) = local_text(charset) else {
            self.native.open_blob(id, charset)?;
            return self.consume();
        };
        // Raw bytes never exceed their UTF-8 form, so the raw read stops early
        // and the converted text is checked against the same documented limit.
        self.native.open_blob(id, 0)?;
        let text = convert(self.consume()?);
        if text.len() as u64 > self.limit {
            return Err(Error::new(
                ErrorKind::ResultLimit,
                "read_blob",
                "BLOB exceeds max_blob_bytes",
            ));
        }
        Ok(text)
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
