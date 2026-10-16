use crate::{ffi, Connection, NativeError};
use std::ffi::CString;

pub const ZONE_CAPACITY: usize = 128;

/// Owned UTC and official local representation. Dates are unused for TIME TZ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonedValue {
    pub utc_date: i32,
    pub utc_time: u32,
    pub local_date: i32,
    pub local_time: u32,
    pub zone: String,
}

fn size(kind: u32) -> Result<usize, NativeError> {
    match kind {
        32754 => Ok(12),
        32756 => Ok(8),
        _ => Err(NativeError::InvalidInput("Unsupported zoned type")),
    }
}

impl Connection {
    pub fn decode_zoned(&mut self, kind: u32, data: &[u8]) -> Result<ZonedValue, NativeError> {
        self.check()?;
        if data.len() != size(kind)? {
            return Err(NativeError::InvalidInput("Invalid zoned buffer length"));
        }
        let mut out = ffi::RawZoned {
            utc_date: 0,
            utc_time: 0,
            local_date: 0,
            local_time: 0,
            zone: [0; ZONE_CAPACITY],
        };
        let mut status = ffi::RawStatus::default();
        // SAFETY: exact width checked above; bridge copies into aligned SDK
        // storage and writes only the matching repr(C) output.
        unsafe {
            ffi::fbfast_decode_zoned(
                self.handle.as_ptr(),
                kind,
                data.as_ptr(),
                &mut out,
                &mut status,
            );
        }
        status.check("decode_timezone")?;
        let length = out
            .zone
            .iter()
            .position(|b| *b == 0)
            .ok_or(NativeError::InvalidInput("Unterminated time zone"))?;
        let zone = std::str::from_utf8(&out.zone[..length])
            .map_err(|_| NativeError::InvalidInput("Invalid time zone encoding"))?;
        if zone.is_empty() {
            return Err(NativeError::InvalidInput("Native time zone is empty"));
        }
        if zone.starts_with("GMT*") {
            return Err(NativeError::InvalidInput("Native time zone ID is unknown"));
        }
        if out.utc_time >= 864_000_000 || out.local_time >= 864_000_000 {
            return Err(NativeError::InvalidInput(
                "Native zoned time is out of range",
            ));
        }
        Ok(ZonedValue {
            utc_date: out.utc_date,
            utc_time: out.utc_time,
            local_date: out.local_date,
            local_time: out.local_time,
            zone: zone.into(),
        })
    }

    pub fn encode_zoned(
        &mut self,
        kind: u32,
        date: i32,
        time: u32,
        zone: &str,
    ) -> Result<Vec<u8>, NativeError> {
        self.check()?;
        let mut data = vec![0; size(kind)?];
        if time >= 864_000_000
            || (kind == 32754 && !(-678_575..=2_973_483).contains(&date))
            || zone.is_empty()
            || zone.len() >= ZONE_CAPACITY
        {
            return Err(NativeError::InvalidInput("Invalid zoned parameter"));
        }
        let zone =
            CString::new(zone).map_err(|_| NativeError::InvalidInput("Invalid time zone"))?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: exact writable width, terminated zone and all buffers live
        // for the synchronous codec call on the owning worker.
        unsafe {
            ffi::fbfast_encode_zoned(
                self.handle.as_ptr(),
                kind,
                date,
                time,
                zone.as_ptr(),
                data.as_mut_ptr(),
                &mut status,
            );
        }
        status.check("bind_timezone")?;
        Ok(data)
    }
}
