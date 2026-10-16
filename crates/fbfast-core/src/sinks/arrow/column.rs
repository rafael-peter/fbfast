use super::conversion;
use crate::{decode::Value, error::Error};
use arrow_array::{builder::*, ArrayRef};
use arrow_buffer::i256;
use arrow_schema::{DataType, TimeUnit};
use std::sync::Arc;

pub(super) enum ColumnBuilder {
    Bool(BooleanBuilder),
    I16(Int16Builder),
    I32(Int32Builder),
    I64(Int64Builder),
    D128(Decimal128Builder),
    D256(Decimal256Builder),
    F32(Float32Builder),
    F64(Float64Builder),
    Text(LargeStringBuilder),
    Bytes(LargeBinaryBuilder),
    Date(Date32Builder),
    Time(Time64MicrosecondBuilder),
    Stamp(TimestampMicrosecondBuilder),
    TimeTz(StructBuilder),
    StampTz(StructBuilder),
}

macro_rules! each {
    ($value:expr, $binding:ident, $action:expr) => {
        match $value {
            ColumnBuilder::Bool($binding) => $action,
            ColumnBuilder::I16($binding) => $action,
            ColumnBuilder::I32($binding) => $action,
            ColumnBuilder::I64($binding) => $action,
            ColumnBuilder::D128($binding) => $action,
            ColumnBuilder::D256($binding) => $action,
            ColumnBuilder::F32($binding) => $action,
            ColumnBuilder::F64($binding) => $action,
            ColumnBuilder::Text($binding) => $action,
            ColumnBuilder::Bytes($binding) => $action,
            ColumnBuilder::Date($binding) => $action,
            ColumnBuilder::Time($binding) => $action,
            ColumnBuilder::Stamp($binding) => $action,
            ColumnBuilder::TimeTz($binding) => $action,
            ColumnBuilder::StampTz($binding) => $action,
        }
    };
}

impl ColumnBuilder {
    pub fn new(dtype: &DataType, rows: usize, bytes: usize) -> Result<Self, Error> {
        use DataType::*;
        // A one-row BLOB batch must not retain a full batch_bytes allocation
        // per tiny cell. Reserve a bounded initial estimate, then grow as used.
        let bytes = bytes.min(rows.saturating_mul(64));
        Ok(match dtype {
            Boolean => Self::Bool(BooleanBuilder::with_capacity(rows)),
            Int16 => Self::I16(Int16Builder::with_capacity(rows)),
            Int32 => Self::I32(Int32Builder::with_capacity(rows)),
            Int64 => Self::I64(Int64Builder::with_capacity(rows)),
            Decimal128(_, _) => {
                Self::D128(Decimal128Builder::with_capacity(rows).with_data_type(dtype.clone()))
            }
            Decimal256(_, _) => {
                Self::D256(Decimal256Builder::with_capacity(rows).with_data_type(dtype.clone()))
            }
            Float32 => Self::F32(Float32Builder::with_capacity(rows)),
            Float64 => Self::F64(Float64Builder::with_capacity(rows)),
            LargeUtf8 => Self::Text(LargeStringBuilder::with_capacity(rows, bytes)),
            LargeBinary => Self::Bytes(LargeBinaryBuilder::with_capacity(rows, bytes)),
            Date32 => Self::Date(Date32Builder::with_capacity(rows)),
            Time64(TimeUnit::Microsecond) => {
                Self::Time(Time64MicrosecondBuilder::with_capacity(rows))
            }
            Timestamp(TimeUnit::Microsecond, _) => Self::Stamp(
                TimestampMicrosecondBuilder::with_capacity(rows).with_data_type(dtype.clone()),
            ),
            Struct(fields) => {
                let builder = StructBuilder::new(
                    fields.clone(),
                    vec![
                        make_builder(fields[0].data_type(), rows),
                        Box::new(LargeStringBuilder::with_capacity(rows, bytes)),
                    ],
                );
                if fields[0].name() == "utc" {
                    Self::StampTz(builder)
                } else {
                    Self::TimeTz(builder)
                }
            }
            _ => return Err(conversion("Unsupported Arrow builder type")),
        })
    }

    pub fn append(&mut self, value: Value) -> Result<(), Error> {
        if value == Value::Null {
            return self.null();
        }
        let invalid = || conversion("Decoded value does not fit the Arrow schema");
        match (self, value) {
            (Self::Bool(b), Value::Bool(v)) => b.append_value(v),
            (Self::I16(b), Value::Int(v)) => {
                b.append_value(i16::try_from(v).map_err(|_| invalid())?)
            }
            (Self::I32(b), Value::Int(v)) => {
                b.append_value(i32::try_from(v).map_err(|_| invalid())?)
            }
            (Self::I64(b), Value::Int(v)) => {
                b.append_value(i64::try_from(v).map_err(|_| invalid())?)
            }
            (Self::D128(b), Value::Decimal { coefficient, .. } | Value::Int(coefficient)) => {
                b.append_value(coefficient)
            }
            (Self::D256(b), Value::Decimal { coefficient, .. } | Value::Int(coefficient)) => {
                b.append_value(i256::from_i128(coefficient))
            }
            (Self::F32(b), Value::Float(v)) => b.append_value(v as f32),
            (Self::F64(b), Value::Float(v)) => b.append_value(v),
            (Self::Text(b), Value::Text(v) | Value::DecFloat(v)) => b.append_value(v),
            (Self::Text(b), Value::Int(v)) => b.append_value(v.to_string()),
            (Self::Text(b), Value::Decimal { coefficient, scale }) => {
                b.append_value(format!("{coefficient}E{scale}"))
            }
            (Self::Bytes(b), Value::Bytes(v)) => b.append_value(v),
            (Self::Date(b), Value::Date(v)) => b.append_value(day(v)?),
            (Self::Time(b), Value::Time(v)) => b.append_value(micros(v)?),
            (Self::Stamp(b), Value::Timestamp { date, time }) => {
                b.append_value(timestamp(date, time)?)
            }
            (Self::TimeTz(b), Value::TimeTz { utc_time, zone }) => {
                b.field_builder::<Time64MicrosecondBuilder>(0)
                    .ok_or_else(invalid)?
                    .append_value(micros(utc_time)?);
                b.field_builder::<LargeStringBuilder>(1)
                    .ok_or_else(invalid)?
                    .append_value(zone);
                b.append(true);
            }
            (Self::StampTz(b), Value::TimestampTz(v)) => {
                b.field_builder::<TimestampMicrosecondBuilder>(0)
                    .ok_or_else(invalid)?
                    .append_value(timestamp(v.utc_date, v.utc_time)?);
                b.field_builder::<LargeStringBuilder>(1)
                    .ok_or_else(invalid)?
                    .append_value(&v.zone);
                b.append(true);
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }

    fn null(&mut self) -> Result<(), Error> {
        match self {
            Self::TimeTz(b) => {
                b.field_builder::<Time64MicrosecondBuilder>(0)
                    .ok_or_else(|| conversion("Invalid zoned builder"))?
                    .append_null();
                b.field_builder::<LargeStringBuilder>(1)
                    .ok_or_else(|| conversion("Invalid zoned builder"))?
                    .append_null();
                b.append(false);
            }
            Self::StampTz(b) => {
                b.field_builder::<TimestampMicrosecondBuilder>(0)
                    .ok_or_else(|| conversion("Invalid zoned builder"))?
                    .append_null();
                b.field_builder::<LargeStringBuilder>(1)
                    .ok_or_else(|| conversion("Invalid zoned builder"))?
                    .append_null();
                b.append(false);
            }
            value => {
                each!(value, b, b.append_null());
            }
        }
        Ok(())
    }

    pub fn finish(&mut self) -> ArrayRef {
        each!(self, b, Arc::new(b.finish()))
    }
}

fn day(value: i32) -> Result<i32, Error> {
    if !(-678_575..=2_973_483).contains(&value) {
        return Err(conversion("Date exceeds SQL domain"));
    }
    Ok(value - 40_587)
}
fn micros(value: u32) -> Result<i64, Error> {
    if value >= 864_000_000 {
        return Err(conversion("Invalid SQL time"));
    }
    Ok(value as i64 * 100)
}
fn timestamp(date: i32, time: u32) -> Result<i64, Error> {
    Ok(day(date)? as i64 * 86_400_000_000 + micros(time)?)
}
