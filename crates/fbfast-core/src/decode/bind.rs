use super::Value;
use crate::error::{Error, ErrorKind};
use fbfast_native::Metadata;

pub fn encode(schema: &Metadata, parameters: &[Value]) -> Result<Vec<u8>, Error> {
    encode_with(schema, parameters, &mut |_, _| {
        Err(super::plan::conversion(
            "This type requires native encoding",
        ))
    })
}

pub fn encode_with(
    schema: &Metadata,
    parameters: &[Value],
    native: &mut impl FnMut(u32, &Value) -> Result<Vec<u8>, Error>,
) -> Result<Vec<u8>, Error> {
    if schema.columns.len() != parameters.len() {
        return Err(Error::new(
            ErrorKind::DataConversion,
            "bind",
            format!(
                "Expected {} parameters, received {}",
                schema.columns.len(),
                parameters.len()
            ),
        ));
    }
    let mut message = vec![0u8; schema.message_length as usize];
    for (index, (c, value)) in schema.columns.iter().zip(parameters).enumerate() {
        let invalid = || {
            Error::new(
                ErrorKind::DataConversion,
                "bind",
                format!(
                    "Parameter {} is incompatible with Firebird type {} or exceeds its range/scale",
                    index + 1,
                    c.sql_type
                ),
            )
        };
        if *value == Value::Null {
            write(&mut message, c.null_offset as usize, &(-1i16).to_ne_bytes())?;
            continue;
        }
        let offset = c.offset as usize;
        if matches!(c.sql_type, 32760 | 32762 | 32754 | 32756) {
            // Utility errors may quote the input. Report only parameter index/type.
            let data = native(c.sql_type, value).map_err(|_| invalid())?;
            if data.len() != c.length as usize {
                return Err(invalid());
            }
            write(&mut message, offset, &data)?;
            continue;
        }
        let integer = match value {
            Value::Int(n) => Some(rescale(*n, 0, c.scale).ok_or_else(invalid)?),
            Value::Decimal { coefficient, scale } => {
                Some(rescale(*coefficient, *scale, c.scale).ok_or_else(invalid)?)
            }
            Value::DecFloat(text) => Some(
                super::decimal::DecimalParts::parse(text)
                    .and_then(|value| value.scaled_i128(c.scale))
                    .ok_or_else(invalid)?,
            ),
            _ => None,
        };
        let data = match (c.sql_type, value) {
            (500, _) if integer.is_some() => i16::try_from(integer.ok_or_else(invalid)?)
                .map_err(|_| invalid())?
                .to_ne_bytes()
                .to_vec(),
            (496, _) if integer.is_some() => i32::try_from(integer.ok_or_else(invalid)?)
                .map_err(|_| invalid())?
                .to_ne_bytes()
                .to_vec(),
            (580, _) if integer.is_some() => i64::try_from(integer.ok_or_else(invalid)?)
                .map_err(|_| invalid())?
                .to_ne_bytes()
                .to_vec(),
            (32752, _) if integer.is_some() => integer.ok_or_else(invalid)?.to_ne_bytes().to_vec(),
            (480, Value::Float(n)) => n.to_ne_bytes().to_vec(),
            (482, Value::Float(n)) if !n.is_finite() || (*n as f32).is_finite() => {
                (*n as f32).to_ne_bytes().to_vec()
            }
            (32764, Value::Bool(b)) => vec![u8::from(*b)],
            (570, Value::Date(n)) => n.to_ne_bytes().to_vec(),
            (560, Value::Time(n)) if *n < 864_000_000 => n.to_ne_bytes().to_vec(),
            (510, Value::Timestamp { date, time }) if *time < 864_000_000 => {
                [date.to_ne_bytes(), time.to_ne_bytes()].concat()
            }
            (448 | 452, Value::Text(s)) if c.charset == 4 => s.as_bytes().to_vec(),
            (448 | 452, Value::Bytes(b)) if c.charset == 0 || c.charset == 1 => b.clone(),
            _ => return Err(invalid()),
        };
        if data.len() > c.length as usize
            || (matches!(c.sql_type, 448 | 452)
                && c.charset == 4
                && matches!(value, Value::Text(s) if s.chars().count() > c.length as usize / 4))
        {
            return Err(invalid());
        }
        if c.sql_type == 448 {
            let length = u16::try_from(data.len()).map_err(|_| invalid())?;
            write(&mut message, offset, &length.to_ne_bytes())?;
            write(&mut message, offset + 2, &data)?;
        } else {
            if c.sql_type == 452 {
                let padding = if matches!(value, Value::Text(_)) {
                    b' '
                } else {
                    0
                };
                let end = offset + c.length as usize;
                message
                    .get_mut(offset..end)
                    .ok_or_else(invalid)?
                    .fill(padding);
            }
            write(&mut message, offset, &data)?;
        }
    }
    Ok(message)
}

pub fn rescale(coefficient: i128, from: i32, to: i32) -> Option<i128> {
    if coefficient == 0 {
        return Some(0);
    }
    let shift = from.checked_sub(to)?;
    let factor = 10i128.checked_pow(shift.unsigned_abs())?;
    if shift >= 0 {
        coefficient.checked_mul(factor)
    } else if coefficient % factor == 0 {
        Some(coefficient / factor)
    } else {
        None
    }
}

fn write(message: &mut [u8], offset: usize, data: &[u8]) -> Result<(), Error> {
    message
        .get_mut(offset..offset + data.len())
        .ok_or_else(|| {
            Error::new(
                ErrorKind::DataConversion,
                "bind",
                "Parameter exceeds message bounds",
            )
        })?
        .copy_from_slice(data);
    Ok(())
}
