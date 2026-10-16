use super::{blob::Blobs, control::Cancellation, cursor::check_deadline};
use crate::{decode::NativeReader, error::Error};
use fbfast_native::Connection;
use std::time::Instant;

pub(super) struct NativeValues<'a> {
    pub native: &'a mut Connection,
    pub cancellation: &'a Cancellation,
    pub deadline: Option<Instant>,
    pub limit: u64,
}

impl NativeReader for NativeValues<'_> {
    fn zoned(&mut self, kind: u32, data: &[u8]) -> Result<fbfast_native::ZonedValue, Error> {
        self.cancellation.check()?;
        check_deadline(self.deadline)?;
        self.native.decode_zoned(kind, data).map_err(Error::from)
    }
    fn blob(&mut self, id: [u8; 8], charset: u32) -> Result<Vec<u8>, Error> {
        Blobs {
            native: self.native,
            cancellation: self.cancellation,
            deadline: self.deadline,
            limit: self.limit,
        }
        .read(id, charset)
    }
    fn decimal(&mut self, kind: u32, data: &[u8]) -> Result<String, Error> {
        self.cancellation.check()?;
        check_deadline(self.deadline)?;
        self.native.decimal_text(kind, data).map_err(Error::from)
    }
}
