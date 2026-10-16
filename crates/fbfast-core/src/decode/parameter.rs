use super::{decimal, plan::conversion, Value};
use crate::error::Error;
use fbfast_native::Connection;

pub fn encode_native(
    connection: &mut Connection,
    kind: u32,
    value: &Value,
) -> Result<Vec<u8>, Error> {
    if matches!(kind, 32760 | 32762) {
        let text = match value {
            Value::Int(n) => n.to_string(),
            Value::Decimal { coefficient, scale } => format!("{coefficient}E{scale}"),
            Value::DecFloat(text) => text.clone(),
            _ => return Err(conversion("Invalid decimal parameter type")),
        };
        return decimal::encode(connection, kind, &text);
    }
    let data = match (kind, value) {
        (32754, Value::TimestampTz(value)) => {
            connection.encode_zoned(kind, value.utc_date, value.utc_time, &value.zone)?
        }
        (32756, Value::TimeTz { utc_time, zone }) => {
            connection.encode_zoned(kind, 0, *utc_time, zone)?
        }
        _ => return Err(conversion("Invalid zoned parameter type")),
    };
    let decoded = connection.decode_zoned(kind, &data)?;
    if let Value::TimestampTz(expected) = value {
        // Preserve the caller's instant and wall time. This catches nonexistent
        // local times and disagreements between Python and the client's tzdata.
        if decoded.utc_date != expected.utc_date
            || decoded.utc_time != expected.utc_time
            || decoded.local_date != expected.local_date
            || decoded.local_time != expected.local_time
        {
            return Err(conversion(
                "Time zone cannot represent the parameter exactly",
            ));
        }
    }
    Ok(data)
}
