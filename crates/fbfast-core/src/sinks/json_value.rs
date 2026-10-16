use super::json::{conversion, JsonOptions};
use crate::{decode::Value, error::Error};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use serde_json::ser::{CompactFormatter, Formatter};

pub(super) fn serialized(out: &mut Vec<u8>, value: &impl Serialize) -> Result<(), Error> {
    serde_json::to_writer(out, value).map_err(|_| conversion("JSON serialization failed"))
}

pub(super) fn value(
    out: &mut Vec<u8>,
    value: &Value,
    sql_type: u32,
    options: JsonOptions,
) -> Result<(), Error> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(value) => serialized(out, value)?,
        Value::Int(value) if options.safe_integers && matches!(sql_type, 580 | 32752) => {
            serialized(out, &value.to_string())?;
        }
        Value::Int(value) => serialized(out, value)?,
        Value::Decimal { coefficient, scale } => {
            if options.string_decimals {
                serialized(out, &decimal(*coefficient, *scale))?;
            } else {
                // Both components are native integers. No float conversion, rounding,
                // untrusted raw JSON, or full serde_json::Value tree is involved.
                CompactFormatter
                    .write_number_str(out, &format!("{coefficient}e{scale}"))
                    .map_err(|_| conversion("JSON numeric serialization failed"))?;
            }
        }
        Value::DecFloat(text) => {
            let decimal = crate::decode::decimal::DecimalParts::parse(text)
                .ok_or_else(|| conversion("Invalid native DECFLOAT text"))?;
            if let Some(number) = decimal.json_number() {
                if options.string_decimals {
                    serialized(out, text)?;
                } else {
                    CompactFormatter
                        .write_number_str(out, &number)
                        .map_err(|_| conversion("JSON numeric serialization failed"))?;
                }
            } else if options.null_non_finite {
                out.extend_from_slice(b"null");
            } else {
                return Err(conversion(
                    "Non-finite DECFLOAT requires json_non_finite='null'",
                ));
            }
        }
        Value::Float(value) if value.is_finite() => serialized(out, value)?,
        Value::Float(_) if options.null_non_finite => out.extend_from_slice(b"null"),
        Value::Float(_) => {
            return Err(conversion(
                "Non-finite number requires json_non_finite='null'",
            ))
        }
        Value::Text(value) => serialized(out, value)?,
        Value::Bytes(value) if options.base64_binary => serialized(out, &STANDARD.encode(value))?,
        Value::Bytes(_) => return Err(conversion("Binary value requires json_binary='base64'")),
        Value::Date(value) => serialized(out, &date(*value)?)?,
        Value::Time(value) => serialized(out, &time(*value)?)?,
        Value::TimeTz { utc_time, zone } => {
            out.extend_from_slice(b"{\"utc_time\":");
            serialized(out, &time(*utc_time)?)?;
            out.extend_from_slice(b",\"zone\":");
            serialized(out, zone)?;
            out.push(b'}');
        }
        Value::TimestampTz(value) => {
            out.extend_from_slice(b"{\"utc\":");
            serialized(
                out,
                &format!("{}T{}Z", date(value.utc_date)?, time(value.utc_time)?),
            )?;
            out.extend_from_slice(b",\"zone\":");
            serialized(out, &value.zone)?;
            out.push(b'}');
        }
        Value::Timestamp {
            date: day,
            time: ticks,
        } => {
            serialized(out, &format!("{}T{}", date(*day)?, time(*ticks)?))?;
        }
    }
    Ok(())
}

fn decimal(coefficient: i128, scale: i32) -> String {
    // SQL fixed decimals have small scales; retain a bounded exact scientific
    // representation for any larger exponent rather than allocating gigabytes.
    if !(-38..=38).contains(&scale) {
        return format!("{coefficient}E{scale:+}");
    }
    let digits = coefficient.unsigned_abs().to_string();
    let sign = if coefficient < 0 { "-" } else { "" };
    if scale >= 0 {
        return format!("{sign}{digits}{}", "0".repeat(scale as usize));
    }
    let places = (-scale) as usize;
    if places < digits.len() {
        let (whole, fractional) = digits.split_at(digits.len() - places);
        format!("{sign}{whole}.{fractional}")
    } else {
        format!("{sign}0.{}{digits}", "0".repeat(places - digits.len()))
    }
}

fn date(value: i32) -> Result<String, Error> {
    let julian = value
        .checked_add(2_400_001)
        .ok_or_else(|| conversion("Date out of range"))?;
    let day = time::Date::from_julian_day(julian).map_err(|_| conversion("Date out of range"))?;
    if day.year() < 1 {
        return Err(conversion("Date out of range"));
    }
    Ok(day.to_string())
}
fn time(ticks: u32) -> Result<String, Error> {
    if ticks >= 864_000_000 {
        return Err(conversion("Native time out of range"));
    }
    let seconds = ticks / 10_000;
    let mut text = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if !ticks.is_multiple_of(10_000) {
        text.push_str(&format!(".{:06}", ticks % 10_000 * 100));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimal_and_temporal_boundaries() {
        assert_eq!(decimal(i128::MIN, 0), i128::MIN.to_string());
        assert_eq!(decimal(-1, -4), "-0.0001");
        assert_eq!(decimal(-123456789012, -4), "-12345678.9012");
        assert_eq!(decimal(0, -4), "0.0000");
        assert_eq!(decimal(123, 2), "12300");
        assert_eq!(date(0).unwrap(), "1858-11-17");
        assert_eq!(date(1 - 678576).unwrap(), "0001-01-01");
        assert_eq!(date(3652059 - 678576).unwrap(), "9999-12-31");
        assert!(date(i32::MAX).is_err());
        assert_eq!(time(863_999_999).unwrap(), "23:59:59.999900");
        assert!(time(864_000_000).is_err());
    }
    #[test]
    fn non_finite_binary_and_schema_based_integers() {
        let mut out = Vec::new();
        let options = JsonOptions::default();
        value(&mut out, &Value::Int(1), 580, options).unwrap();
        assert_eq!(out, br#""1""#);
        out.clear();
        value(&mut out, &Value::Int(1), 496, options).unwrap();
        assert_eq!(out, b"1");
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(value(&mut out, &Value::Float(number), 482, options).is_err());
            out.clear();
            value(
                &mut out,
                &Value::Float(number),
                482,
                JsonOptions {
                    null_non_finite: true,
                    ..options
                },
            )
            .unwrap();
            assert_eq!(out, b"null");
        }
        assert!(value(&mut out, &Value::Bytes(vec![0, 255]), 452, options).is_err());
        out.clear();
        value(
            &mut out,
            &Value::Bytes(vec![0, 255]),
            452,
            JsonOptions {
                base64_binary: true,
                ..options
            },
        )
        .unwrap();
        assert_eq!(out, br#""AP8=""#);
    }
}
