//! Integer attachment information: wire statistics and protocol capabilities.
use crate::{ffi, Connection, NativeError};

/// Firebird attachment information tags (`firebird/impl/inf_pub.h`).
pub mod item {
    pub const PROTOCOL_VERSION: u8 = 137;
    pub const WIRE_OUT_BYTES: u8 = 152;
    pub const WIRE_IN_BYTES: u8 = 153;
    pub const WIRE_SND_PACKETS: u8 = 154;
    pub const WIRE_RCV_PACKETS: u8 = 155;
    pub const WIRE_SND_BYTES: u8 = 156;
    pub const WIRE_RCV_BYTES: u8 = 157;
    pub const WIRE_ROUNDTRIPS: u8 = 158;
    pub const MAX_BLOB_CACHE_SIZE: u8 = 159;
    pub const MAX_INLINE_BLOB_SIZE: u8 = 160;
}

/// Cumulative client-side counters kept by the Remote provider for one attachment.
/// `*_bytes` are physical (after compression); `logical_*` are before compression.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WireStats {
    pub roundtrips: u64,
    pub sent_packets: u64,
    pub received_packets: u64,
    pub sent_bytes: u64,
    pub received_bytes: u64,
    pub logical_sent_bytes: u64,
    pub logical_received_bytes: u64,
}

impl WireStats {
    /// Counters accumulated since `earlier` (saturating, never negative).
    pub fn since(&self, earlier: &Self) -> Self {
        Self {
            roundtrips: self.roundtrips.saturating_sub(earlier.roundtrips),
            sent_packets: self.sent_packets.saturating_sub(earlier.sent_packets),
            received_packets: self
                .received_packets
                .saturating_sub(earlier.received_packets),
            sent_bytes: self.sent_bytes.saturating_sub(earlier.sent_bytes),
            received_bytes: self.received_bytes.saturating_sub(earlier.received_bytes),
            logical_sent_bytes: self
                .logical_sent_bytes
                .saturating_sub(earlier.logical_sent_bytes),
            logical_received_bytes: self
                .logical_received_bytes
                .saturating_sub(earlier.logical_received_bytes),
        }
    }
}

const WIRE_ITEMS: [u8; 7] = [
    item::WIRE_ROUNDTRIPS,
    item::WIRE_SND_PACKETS,
    item::WIRE_RCV_PACKETS,
    item::WIRE_SND_BYTES,
    item::WIRE_RCV_BYTES,
    item::WIRE_OUT_BYTES,
    item::WIRE_IN_BYTES,
];

impl Connection {
    /// Reads integer info items; `None` marks an item the provider did not report.
    pub fn info(&mut self, items: &[u8]) -> Result<Vec<Option<u64>>, NativeError> {
        self.check()?;
        if items.is_empty() || items.len() > 32 {
            return Err(NativeError::InvalidInput("Invalid attachment info request"));
        }
        let mut values = vec![u64::MAX; items.len()];
        let mut status = ffi::RawStatus::default();
        // SAFETY: live exclusively owned connection; both buffers hold `items.len()` entries.
        unsafe {
            ffi::fbfast_attachment_info(
                self.handle.as_ptr(),
                items.as_ptr(),
                items.len() as u32,
                values.as_mut_ptr(),
                &mut status,
            );
        }
        status.check("info")?;
        Ok(values
            .into_iter()
            .map(|value| (value != u64::MAX).then_some(value))
            .collect())
    }

    /// Wire counters for this attachment, or `None` from a non-remote provider.
    pub fn wire_stats(&mut self) -> Result<Option<WireStats>, NativeError> {
        let values = self.info(&WIRE_ITEMS)?;
        let Some(roundtrips) = values[0] else {
            return Ok(None);
        };
        let at = |index: usize| values[index].unwrap_or(0);
        Ok(Some(WireStats {
            roundtrips,
            sent_packets: at(1),
            received_packets: at(2),
            sent_bytes: at(3),
            received_bytes: at(4),
            logical_sent_bytes: at(5),
            logical_received_bytes: at(6),
        }))
    }
}
