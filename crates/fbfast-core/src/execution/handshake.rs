use super::{
    control::{Cancellation, QueryOptions},
    ExecutionConnection,
};
use crate::{
    decode::Value,
    error::{Error, ErrorKind},
};

const SESSION_SQL: &str = r#"
SELECT CAST(RDB$GET_CONTEXT('SYSTEM', 'ENGINE_VERSION') AS VARCHAR(64)),
       CAST(CURRENT_ROLE AS VARCHAR(63)), TRIM(C.RDB$CHARACTER_SET_NAME),
       TRIM(A.MON$SESSION_TIMEZONE), T.MON$READ_ONLY, T.MON$ISOLATION_MODE,
       A.MON$WIRE_ENCRYPTED, A.MON$WIRE_COMPRESSED, A.MON$REMOTE_PROTOCOL
FROM MON$ATTACHMENTS A
JOIN RDB$CHARACTER_SETS C ON C.RDB$CHARACTER_SET_ID = A.MON$CHARACTER_SET_ID
CROSS JOIN MON$TRANSACTIONS T
WHERE A.MON$ATTACHMENT_ID = CURRENT_CONNECTION
  AND T.MON$TRANSACTION_ID = CURRENT_TRANSACTION
"#;

/// Observed capabilities, not assumptions from the requested DPB settings.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub server_version: String,
    pub role: String,
    pub charset: String,
    pub time_zone: String,
    pub wire_encrypted: bool,
    pub wire_compressed: bool,
    pub network_protocol: String,
    /// Negotiated wire protocol (19 = Firebird 5.0.3+ with inline BLOBs).
    pub protocol_version: u32,
    /// Small BLOBs arrive with their row instead of costing extra round trips.
    pub inline_blobs: bool,
    pub max_inline_blob_size: u32,
    pub blob_cache_size: u64,
}

/// Inline BLOB transfer was introduced with wire protocol 19 (Firebird 5.0.3).
const INLINE_BLOB_PROTOCOL: u32 = 19;

impl ExecutionConnection {
    pub(crate) fn verify_session(
        &mut self,
        deadline: std::time::Instant,
    ) -> Result<SessionInfo, Error> {
        // The internal query has fixed-width fields and at most one result row.
        let limits = (self.config.max_result_rows, self.config.max_result_bytes);
        self.config.max_result_rows = Some(1);
        self.config.max_result_bytes = None;
        let result = self.fetch_with(
            SESSION_SQL,
            &[],
            &QueryOptions {
                first: true,
                unique_names: false,
                prevalidated: false,
                deadline: Some(deadline),
                cancellation: &Cancellation::default(),
                cleaning: &|| Ok(false),
            },
        );
        (self.config.max_result_rows, self.config.max_result_bytes) = limits;
        let result = result?;
        let row = result
            .row(0)
            .ok_or_else(|| mismatch("Server did not return session identity"))?;
        let mut info = SessionInfo {
            server_version: text(row, 0)?,
            role: text(row, 1)?,
            charset: text(row, 2)?,
            time_zone: text(row, 3)?,
            wire_encrypted: boolean(row, 6)?,
            wire_compressed: boolean(row, 7)?,
            network_protocol: text(row, 8)?,
            protocol_version: 0,
            inline_blobs: false,
            max_inline_blob_size: 0,
            blob_cache_size: 0,
        };
        self.read_protocol(&mut info)?;
        if !info.server_version.starts_with("5.0.") {
            return Err(Error::new(
                ErrorKind::UnsupportedServer,
                "attach",
                "Only Firebird 5.0.x is supported",
            ));
        }
        let expected = self
            .config
            .role
            .as_deref()
            .map(normalize_role)
            .unwrap_or_else(|| "NONE".into());
        if info.role != expected {
            return Err(Error::new(
                ErrorKind::RoleNotActive,
                "attach",
                "Requested role is not active",
            ));
        }
        if !info.charset.eq_ignore_ascii_case(&self.config.charset) {
            return Err(mismatch(
                "Effective connection charset differs from the requested charset",
            ));
        }
        if normalize_zone(&info.time_zone) != normalize_zone(&self.config.session_time_zone) {
            return Err(mismatch(
                "Effective session time zone differs from the requested time zone",
            ));
        }
        if row.get(4) != Some(&Value::Int(1)) || row.get(5) != Some(&Value::Int(4)) {
            return Err(Error::new(
                ErrorKind::ReadOnlyViolation,
                "attach",
                "Handshake requires READ ONLY and READ COMMITTED READ CONSISTENCY",
            ));
        }
        if !matches!(info.network_protocol.as_str(), "TCPv4" | "TCPv6") {
            return Err(mismatch("Only remote TCP transport is supported"));
        }
        if (self.config.wire_crypt == "required" && !info.wire_encrypted)
            || (self.config.wire_crypt == "disabled" && info.wire_encrypted)
        {
            return Err(mismatch(
                "Effective wire encryption violates the configured policy",
            ));
        }
        // Compression is negotiated with the server. Record the actual outcome;
        // enabling the client option alone does not prove it is in use.
        Ok(info)
    }
}

impl ExecutionConnection {
    /// Local Remote-provider information; no server round trip.
    fn read_protocol(&mut self, info: &mut SessionInfo) -> Result<(), Error> {
        use fbfast_native::info_item as item;
        let values = self.native.info(&[
            item::PROTOCOL_VERSION,
            item::MAX_INLINE_BLOB_SIZE,
            item::MAX_BLOB_CACHE_SIZE,
        ])?;
        // The protocol number carries FB_PROTOCOL_FLAG (0x8000) on the wire.
        info.protocol_version = values[0].map_or(0, |value| (value & 0x7fff) as u32);
        info.max_inline_blob_size = values[1].unwrap_or(0).min(u32::MAX as u64) as u32;
        info.blob_cache_size = values[2].unwrap_or(0);
        info.inline_blobs =
            info.protocol_version >= INLINE_BLOB_PROTOCOL && info.max_inline_blob_size > 0;
        Ok(())
    }
}

fn mismatch(message: &str) -> Error {
    Error::new(ErrorKind::Query, "attach", message)
}
fn text(row: &[Value], index: usize) -> Result<String, Error> {
    match row.get(index) {
        Some(Value::Text(value)) => Ok(value.trim_end().to_owned()),
        _ => Err(mismatch("Invalid session identity metadata")),
    }
}
fn boolean(row: &[Value], index: usize) -> Result<bool, Error> {
    match row.get(index) {
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(mismatch("Invalid connection capability metadata")),
    }
}

fn normalize_role(role: &str) -> String {
    if role.len() >= 2 && role.starts_with('"') && role.ends_with('"') {
        role[1..role.len() - 1].replace("\"\"", "\"")
    } else {
        role.to_uppercase()
    }
}

fn normalize_zone(zone: &str) -> String {
    let zone = zone.trim();
    let unsigned = zone.strip_prefix(['+', '-']).unwrap_or(zone);
    if let Some((hours, minutes)) = unsigned.split_once(':') {
        if let (Ok(hours), Ok(minutes)) = (hours.parse::<u32>(), minutes.parse::<u32>()) {
            let sign = if zone.starts_with('-') && (hours != 0 || minutes != 0) {
                '-'
            } else {
                '+'
            };
            return format!("{sign}{hours:02}:{minutes:02}");
        }
    }
    zone.to_uppercase()
}
