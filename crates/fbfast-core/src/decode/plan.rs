use super::Value;
use crate::{
    config::Config,
    error::{Error, ErrorKind},
};
use fbfast_native::{Column, Metadata};

#[derive(Debug, Clone)]
pub struct DecodePlan {
    pub schema: Metadata,
    trim_char: bool,
    none_as_utf8: bool,
}

impl DecodePlan {
    pub fn new(schema: Metadata, config: &Config) -> Result<Self, Error> {
        let none_as_utf8 = match config.fallback_charset.as_deref() {
            None => false,
            Some(value) if matches!(value.to_ascii_uppercase().as_str(), "UTF8" | "UTF-8") => true,
            Some(_) => {
                return Err(Error::unsupported(
                    "This decoder currently supports UTF8 fallback only",
                ))
            }
        };
        for column in &schema.columns {
            if !matches!(
                column.sql_type,
                500 | 496 | 580 | 32752 | 482 | 480 | 32764 | 452 | 448 | 570 | 560 | 510
            ) {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "describe",
                    format!(
                        "Firebird type {} is not implemented by the scalar decoder",
                        column.sql_type
                    ),
                ));
            }
        }
        Ok(Self {
            schema,
            trim_char: config.trim_char,
            none_as_utf8,
        })
    }

    pub fn value(&self, index: usize, row: &[u8]) -> Result<Value, Error> {
        let c = self
            .schema
            .columns
            .get(index)
            .ok_or_else(|| conversion("Column index out of range"))?;
        if i16::from_ne_bytes(bytes::<2>(row, c.null_offset as usize)?) != 0 {
            return Ok(Value::Null);
        }
        let offset = c.offset as usize;
        let number = match c.sql_type {
            500 => Some(i16::from_ne_bytes(bytes(row, offset)?) as i128),
            496 => Some(i32::from_ne_bytes(bytes(row, offset)?) as i128),
            580 => Some(i64::from_ne_bytes(bytes(row, offset)?) as i128),
            32752 => Some(i128::from_ne_bytes(bytes(row, offset)?)),
            _ => None,
        };
        if let Some(coefficient) = number {
            return Ok(if c.scale != 0 || c.subtype != 0 {
                Value::Decimal {
                    coefficient,
                    scale: c.scale,
                }
            } else {
                Value::Int(coefficient)
            });
        }
        match c.sql_type {
            482 => Ok(Value::Float(f64::from_ne_bytes(bytes(row, offset)?))),
            480 => Ok(Value::Float(f32::from_ne_bytes(bytes(row, offset)?) as f64)),
            32764 => match bytes::<1>(row, offset)?[0] {
                0 => Ok(Value::Bool(false)),
                1 => Ok(Value::Bool(true)),
                _ => Err(conversion("Invalid BOOLEAN byte")),
            },
            452 | 448 => self.text(c, row),
            570 => Ok(Value::Date(i32::from_ne_bytes(bytes(row, offset)?))),
            560 => Ok(Value::Time(u32::from_ne_bytes(bytes(row, offset)?))),
            510 => Ok(Value::Timestamp {
                date: i32::from_ne_bytes(bytes(row, offset)?),
                time: u32::from_ne_bytes(bytes(row, offset + 4)?),
            }),
            _ => Err(conversion("Unsupported decode plan")),
        }
    }

    fn text(&self, column: &Column, row: &[u8]) -> Result<Value, Error> {
        let mut offset = column.offset as usize;
        let length = if column.sql_type == 448 {
            let size = u16::from_ne_bytes(bytes(row, offset)?) as usize;
            offset += 2;
            if size > column.length as usize {
                return Err(conversion("VARCHAR length exceeds metadata"));
            }
            size
        } else {
            column.length as usize
        };
        let data = row
            .get(offset..offset + length)
            .ok_or_else(|| conversion("String exceeds message bounds"))?;
        if column.charset == 1 || (column.charset == 0 && !self.none_as_utf8) {
            return Ok(Value::Bytes(data.into()));
        }
        if column.charset != 4 && !(column.charset == 0 && self.none_as_utf8) {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "decode",
                "Text charset is not UTF8; no lossy decoding is performed",
            ));
        }
        let mut text = std::str::from_utf8(data).map_err(|_| conversion("Invalid UTF8 text"))?;
        if column.sql_type == 452 && column.charset == 4 {
            // Firebird describes UTF8 CHAR(N) as 4*N bytes and pads the message
            // buffer to that size. Retain exactly N SQL characters, including
            // their spaces; remove only transport padding beyond those N chars.
            let characters = column.length as usize / 4;
            if let Some((end, _)) = text.char_indices().nth(characters) {
                if !text[end..].bytes().all(|b| b == b' ') {
                    return Err(conversion(
                        "CHAR contains data beyond its declared character length",
                    ));
                }
                text = &text[..end];
            }
        }
        let text = if self.trim_char && column.sql_type == 452 {
            text.trim_end_matches(' ')
        } else {
            text
        };
        Ok(Value::Text(text.into()))
    }
}

pub(crate) fn conversion(message: &str) -> Error {
    Error::new(ErrorKind::DataConversion, "decode", message)
}

fn bytes<const N: usize>(row: &[u8], offset: usize) -> Result<[u8; N], Error> {
    row.get(offset..offset + N)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| conversion("Value exceeds message bounds"))
}
