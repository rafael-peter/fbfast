//! JSON writers consume one decoded cell at a time, never a row table or JSON tree.
use crate::{
    decode::{DecodePlan, NativeReader},
    error::{Error, ErrorKind},
};

#[derive(Debug, Clone, Copy)]
pub struct JsonOptions {
    pub records: bool,
    pub safe_integers: bool,
    pub string_decimals: bool,
    pub base64_binary: bool,
    pub null_non_finite: bool,
}
impl Default for JsonOptions {
    fn default() -> Self {
        Self {
            records: true,
            safe_integers: true,
            string_decimals: true,
            base64_binary: false,
            null_non_finite: false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonFormat {
    Array,
    Ndjson,
}

#[derive(Debug)]
pub struct JsonOutput {
    pub data: Vec<u8>,
    pub rows: usize,
}

pub struct JsonWriter {
    pub options: JsonOptions,
}
impl JsonWriter {
    pub fn new(plan: &DecodePlan, options: JsonOptions) -> Result<Self, Error> {
        if options.records {
            let mut names = std::collections::HashSet::new();
            if plan.schema.columns.iter().any(|c| !names.insert(&c.name)) {
                return Err(Error::new(
                    ErrorKind::DataConversion,
                    "describe",
                    "Duplicate column names; provide unique aliases",
                ));
            }
        }
        Ok(Self { options })
    }
    pub fn row(
        &self,
        plan: &DecodePlan,
        message: &[u8],
        out: &mut Vec<u8>,
        native: &mut impl NativeReader,
    ) -> Result<(), Error> {
        out.push(if self.options.records { b'{' } else { b'[' });
        for (index, column) in plan.schema.columns.iter().enumerate() {
            if index != 0 {
                out.push(b',');
            }
            if self.options.records {
                super::json_value::serialized(out, &column.name)?;
                out.push(b':');
            }
            super::json_value::value(
                out,
                &plan.value_with_native(index, message, native)?,
                column.sql_type,
                self.options,
            )?;
        }
        out.push(if self.options.records { b'}' } else { b']' });
        Ok(())
    }
}

pub(super) fn conversion(message: &str) -> Error {
    Error::new(ErrorKind::DataConversion, "json", message)
}
