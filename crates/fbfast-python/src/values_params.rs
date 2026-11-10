//! Python parameters to native values; exact types only, never parsed from text.
use super::{conversion, Converter, MJD_ORDINAL};
use fbfast_core::decode::Value;
use pyo3::{
    prelude::*,
    types::{PyBool, PyBytes, PyFloat, PyInt, PyList, PyString, PyTuple},
};

impl<'py> Converter<'py> {
    pub fn params(&self, params: Option<&Bound<'py, PyAny>>) -> PyResult<Vec<Value>> {
        let Some(params) = params.filter(|p| !p.is_none()) else {
            return Ok(Vec::new());
        };
        if !params.is_instance_of::<PyList>() && !params.is_instance_of::<PyTuple>() {
            return Err(conversion("params must be a list, tuple or None"));
        }
        params
            .try_iter()?
            .map(|value| self.parameter(&value?))
            .collect()
    }

    fn parameter(&self, value: &Bound<'py, PyAny>) -> PyResult<Value> {
        if value.is_none() {
            return Ok(Value::Null);
        }
        if value.is_instance_of::<PyBool>() {
            return Ok(Value::Bool(value.extract()?));
        }
        if value.is_instance_of::<PyInt>() {
            return value
                .extract()
                .map(Value::Int)
                .map_err(|_| conversion("Integer exceeds signed INT128 range"));
        }
        if value.is_instance_of::<PyFloat>() {
            return Ok(Value::Float(value.extract()?));
        }
        if value.is_instance_of::<PyString>() {
            return Ok(Value::Text(value.extract()?));
        }
        if value.is_instance_of::<PyBytes>() {
            return Ok(Value::Bytes(value.extract()?));
        }
        if value.is_instance(&self.decimal)? {
            // Preserve exponent, signed zero and special-value payloads. The
            // prepared SQL type determines exact range/scale validation in core.
            return Ok(Value::DecFloat(value.str()?.extract()?));
        }
        if value.is_instance(&self.datetime)? {
            if !value.getattr("tzinfo")?.is_none() {
                let (utc_date, utc_time, local_date, local_time, zone) = self
                    .timestamp_input
                    .call1((value,))?
                    .extract::<(i32, u32, i32, u32, String)>()?;
                return Ok(Value::TimestampTz(Box::new(
                    fbfast_core::decode::ZonedValue {
                        utc_date,
                        utc_time,
                        local_date,
                        local_time,
                        zone,
                    },
                )));
            }
            return Ok(Value::Timestamp {
                date: ordinal(value)?,
                time: ticks(value)?,
            });
        }
        if value.is_instance(&self.date)? {
            return Ok(Value::Date(ordinal(value)?));
        }
        if value.is_instance(&self.time)? {
            naive(value)?;
            return Ok(Value::Time(ticks(value)?));
        }
        if value.is_instance(&self.zoned_time)? {
            return Ok(Value::TimeTz {
                utc_time: ticks(&value.getattr("utc_time")?)?,
                zone: value.getattr("zone")?.extract()?,
            });
        }
        Err(conversion("Unsupported parameter type"))
    }
}

fn ordinal(value: &Bound<'_, PyAny>) -> PyResult<i32> {
    Ok(value.call_method0("toordinal")?.extract::<i32>()? - MJD_ORDINAL)
}
fn naive(value: &Bound<'_, PyAny>) -> PyResult<()> {
    if !value.getattr("tzinfo")?.is_none() {
        return Err(conversion(
            "Use TimeWithZone for TIME WITH TIME ZONE; aware time has no date for DST rules",
        ));
    }
    Ok(())
}
fn ticks(value: &Bound<'_, PyAny>) -> PyResult<u32> {
    let hour: u32 = value.getattr("hour")?.extract()?;
    let minute: u32 = value.getattr("minute")?.extract()?;
    let second: u32 = value.getattr("second")?.extract()?;
    let microsecond: u32 = value.getattr("microsecond")?.extract()?;
    if !microsecond.is_multiple_of(100) {
        return Err(conversion(
            "Firebird TIME precision is 100 microseconds; value would lose precision",
        ));
    }
    Ok(((hour * 60 + minute) * 60 + second) * 10000 + microsecond / 100)
}
