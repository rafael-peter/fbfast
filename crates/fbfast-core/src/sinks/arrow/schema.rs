use super::{conversion, ArrowMode};
use crate::{
    config::Config,
    decode::DecodePlan,
    error::{Error, ErrorKind},
};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use fbfast_native::Column;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub(super) fn schema(
    plan: &DecodePlan,
    config: &Config,
    mode: ArrowMode,
) -> Result<SchemaRef, Error> {
    let mut names = HashSet::new();
    let fields =
        plan.schema
            .columns
            .iter()
            .map(|column| {
                if !names.insert(&column.name) {
                    return Err(conversion(
                        "Duplicate column names require explicit aliases for Arrow/Polars",
                    ));
                }
                let original = data_type(column, config)?;
                let dtype = match (&original, mode) {
                    (DataType::Decimal256(_, _), ArrowMode::Polars) => return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "describe",
                        "Polars cannot preserve Decimal256; use unsupported='string' explicitly",
                    )),
                    (DataType::Decimal256(_, _), ArrowMode::PolarsString) => DataType::LargeUtf8,
                    _ => original.clone(),
                };
                let metadata = HashMap::from([
                    ("fbfast.sql_type".into(), column.sql_type.to_string()),
                    ("fbfast.scale".into(), column.scale.to_string()),
                    ("fbfast.subtype".into(), column.subtype.to_string()),
                    ("fbfast.charset".into(), column.charset.to_string()),
                    ("fbfast.storage_bytes".into(), column.length.to_string()),
                    ("fbfast.original_arrow_type".into(), original.to_string()),
                ]);
                Ok(Field::new(&column.name, dtype, true).with_metadata(metadata))
            })
            .collect::<Result<Vec<_>, Error>>()?;
    Ok(Arc::new(Schema::new(fields)))
}

fn data_type(c: &Column, config: &Config) -> Result<DataType, Error> {
    use DataType::*;
    if matches!(c.sql_type, 500 | 496 | 580 | 32752) && (c.scale != 0 || c.subtype != 0) {
        let digits = match c.sql_type {
            500 => 5,
            496 => 10,
            580 => 19,
            _ => 39,
        };
        let scale = c
            .scale
            .checked_neg()
            .and_then(|s| i8::try_from(s).ok())
            .ok_or_else(|| conversion("SQL decimal scale exceeds Arrow capacity"))?;
        let precision = digits.max(scale.max(0) as u8);
        if precision > 76 {
            return Err(conversion("SQL decimal precision exceeds Arrow capacity"));
        }
        return Ok(if precision > 38 {
            Decimal256(precision, scale)
        } else {
            Decimal128(precision, scale)
        });
    }
    Ok(match c.sql_type {
        500 => Int16,
        496 => Int32,
        580 => Int64,
        32752 => Decimal256(39, 0),
        32760 | 32762 => LargeUtf8,
        480 => Float64,
        482 => Float32,
        32764 => Boolean,
        448 | 452 => match c.charset {
            4 => LargeUtf8,
            0 if config.fallback_charset.is_some() => LargeUtf8,
            0 | 1 => LargeBinary,
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "describe",
                    "Unsupported Arrow text charset",
                ))
            }
        },
        520 if c.subtype == 0
            || c.charset == 1
            || (c.charset == 0 && config.fallback_charset.is_none()) =>
        {
            LargeBinary
        }
        520 => LargeUtf8,
        570 => Date32,
        560 => Time64(TimeUnit::Microsecond),
        510 => Timestamp(TimeUnit::Microsecond, None),
        32754 => Struct(
            vec![
                Field::new(
                    "utc",
                    Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                    true,
                ),
                Field::new("zone", LargeUtf8, true),
            ]
            .into(),
        ),
        32756 => Struct(
            vec![
                Field::new("utc_time", Time64(TimeUnit::Microsecond), true),
                Field::new("zone", LargeUtf8, true),
            ]
            .into(),
        ),
        _ => {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "describe",
                "Unsupported Arrow column type",
            ))
        }
    })
}
