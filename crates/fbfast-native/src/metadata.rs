use crate::{ffi, Connection, NativeError, StatementId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub sql_type: u32,
    pub length: u32,
    pub charset: u32,
    pub offset: u32,
    pub null_offset: u32,
    pub scale: i32,
    pub subtype: i32,
}

#[derive(Debug, Clone)]
pub struct Metadata {
    pub columns: Vec<Column>,
    pub message_length: u32,
}

impl Connection {
    pub fn metadata(
        &mut self,
        statement: StatementId,
        input: bool,
    ) -> Result<Metadata, NativeError> {
        let id = self.validate_statement(statement)?;
        let input = u32::from(input);
        let mut status = ffi::RawStatus::default();
        // SAFETY: registered statement, exclusive live connection, writable out status.
        let count =
            unsafe { ffi::fbfast_column_count(self.handle.as_ptr(), id, input, &mut status) };
        status.check("metadata")?;
        // SAFETY: same statement remains alive and exclusive across calls.
        let message_length =
            unsafe { ffi::fbfast_message_length(self.handle.as_ptr(), id, input, &mut status) };
        status.check("metadata")?;
        let mut columns = Vec::new();
        for index in 0..count {
            let mut raw = ffi::RawColumn::default();
            // SAFETY: index below describe count; raw and status writable for exact ABI sizes.
            unsafe {
                ffi::fbfast_column(
                    self.handle.as_ptr(),
                    id,
                    input,
                    index,
                    &mut raw,
                    &mut status,
                );
            }
            status.check("metadata")?;
            let name = column_name(&raw)?;
            validate_offsets(&raw, message_length)?;
            columns.push(Column {
                name,
                sql_type: raw.sql_type,
                length: raw.length,
                charset: raw.charset,
                offset: raw.offset,
                null_offset: raw.null_offset,
                scale: raw.scale,
                subtype: raw.subtype,
            });
        }
        Ok(Metadata {
            columns,
            message_length,
        })
    }
}

fn column_name(raw: &ffi::RawColumn) -> Result<String, NativeError> {
    let name = raw
        .name
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8(name).map_err(|_| NativeError::InvalidInput("Column name is not UTF8"))
}

fn validate_offsets(raw: &ffi::RawColumn, message_length: u32) -> Result<(), NativeError> {
    let payload = raw.length as u64 + if raw.sql_type == 448 { 2 } else { 0 };
    if raw.offset as u64 + payload > message_length as u64
        || raw.null_offset as u64 + 2 > message_length as u64
    {
        Err(NativeError::InvalidInput(
            "Metadata offset exceeds message size",
        ))
    } else {
        Ok(())
    }
}

#[cfg(feature = "fuzzing")]
pub fn fuzz_metadata(data: &[u8]) {
    let mut raw = ffi::RawColumn::default();
    for (target, source) in raw.name.iter_mut().zip(data.iter().copied()) {
        *target = source as std::ffi::c_char;
    }
    let number = |start: usize| {
        u32::from_le_bytes(std::array::from_fn(|offset| {
            data.get(start + offset).copied().unwrap_or_default()
        }))
    };
    raw.sql_type = number(0);
    raw.length = number(4);
    raw.offset = number(8);
    raw.null_offset = number(12);
    let _ = column_name(&raw);
    let _ = validate_offsets(&raw, number(16));
}
