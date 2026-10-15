//! Python objects exist only at the binding boundary, after native cleanup.
use crate::errors::from_core;
use fbfast_core::pool::DeliveryBudget;
use fbfast_core::{
    decode::Value,
    error::{Error, ErrorKind},
    sinks::rows::Rows,
};
use pyo3::{
    prelude::*,
    types::{PyBool, PyBytes, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple},
    IntoPyObjectExt,
};
use std::{
    cell::Cell,
    sync::Arc,
    time::{Duration, Instant},
};

const MJD_ORDINAL: i32 = 678576;

pub struct Converter<'py> {
    budget: Option<DeliveryBudget>,
    converted: Cell<usize>,
    last_checkpoint: Cell<Instant>,
    async_control: Option<Arc<crate::async_runtime::Control>>,
    py: Python<'py>,
    decimal: Bound<'py, PyAny>,
    date: Bound<'py, PyAny>,
    time: Bound<'py, PyAny>,
    datetime: Bound<'py, PyAny>,
}

impl<'py> Converter<'py> {
    pub fn new(py: Python<'py>) -> PyResult<Self> {
        let datetime = py.import("datetime")?;
        Ok(Self {
            budget: None,
            converted: Cell::new(0),
            last_checkpoint: Cell::new(Instant::now()),
            async_control: None,
            py,
            decimal: py.import("decimal")?.getattr("Decimal")?,
            date: datetime.getattr("date")?,
            time: datetime.getattr("time")?,
            datetime: datetime.getattr("datetime")?,
        })
    }

    pub fn set_budget(&mut self, budget: DeliveryBudget) {
        self.budget = Some(budget);
    }

    pub fn set_async_control(&mut self, control: Arc<crate::async_runtime::Control>) {
        self.async_control = Some(control);
    }

    pub fn checkpoint(&self) -> PyResult<()> {
        self.py.check_signals()?;
        if let Some(budget) = &self.budget {
            budget.check().map_err(from_core)?;
        }
        if let Some(control) = &self.async_control {
            if let Err(error) = control.check(self.py) {
                if let Some(budget) = &self.budget {
                    budget.cancel();
                }
                return Err(error);
            }
            // A bounded GIL-free interval gives the Python loop a real scheduling
            // opportunity even while millions of small objects are materialized.
            self.py
                .detach(|| std::thread::sleep(Duration::from_micros(100)));
        } else {
            self.py.detach(std::thread::yield_now);
        }
        self.last_checkpoint.set(Instant::now());
        Ok(())
    }

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
            if !value.call_method0("is_finite")?.extract::<bool>()? {
                return Err(conversion(
                    "Non-finite Decimal is not supported by fixed numeric binding",
                ));
            }
            let tuple = value.call_method0("as_tuple")?;
            let sign: u8 = tuple.getattr("sign")?.extract()?;
            let digits: Vec<u8> = tuple.getattr("digits")?.extract()?;
            let scale = tuple
                .getattr("exponent")?
                .extract::<i32>()
                .map_err(|_| conversion("Decimal exponent exceeds native range"))?;
            let mut text = if sign == 1 {
                "-".to_owned()
            } else {
                String::new()
            };
            text.extend(digits.into_iter().map(|digit| char::from(b'0' + digit)));
            let coefficient = text
                .parse::<i128>()
                .map_err(|_| conversion("Decimal coefficient exceeds signed INT128 range"))?;
            return Ok(Value::Decimal { coefficient, scale });
        }
        if value.is_instance(&self.datetime)? {
            naive(value)?;
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
        Err(conversion("Unsupported parameter type"))
    }

    pub fn value(&self, value: &Value) -> PyResult<Py<PyAny>> {
        let count = self.converted.get();
        self.converted.set(count.wrapping_add(1));
        if count.is_multiple_of(512)
            || (self.async_control.is_some()
                && self.last_checkpoint.get().elapsed() >= Duration::from_millis(2))
        {
            self.checkpoint()?;
        }
        match value {
            Value::Null => Ok(self.py.None()),
            Value::Bool(value) => value.into_py_any(self.py),
            Value::Int(value) => value.into_py_any(self.py),
            Value::Float(value) => value.into_py_any(self.py),
            Value::Text(value) => value.into_py_any(self.py),
            Value::Bytes(value) => Ok(PyBytes::new(self.py, value).into_any().unbind()),
            Value::Decimal { coefficient, scale } => {
                let text = coefficient.unsigned_abs().to_string();
                let digits = PyTuple::new(self.py, text.bytes().map(|b| b - b'0'))?;
                Ok(self
                    .decimal
                    .call1(((u8::from(*coefficient < 0), digits, scale),))?
                    .unbind())
            }
            Value::Date(value) => Ok(self
                .date
                .call_method1(
                    "fromordinal",
                    (value
                        .checked_add(MJD_ORDINAL)
                        .ok_or_else(|| conversion("Date out of range"))?,),
                )?
                .unbind()),
            Value::Time(value) => self.time_value(*value),
            Value::Timestamp { date, time } => {
                let date = self.value(&Value::Date(*date))?;
                let time = self.time_value(*time)?;
                Ok(self
                    .datetime
                    .call_method1("combine", (date, time))?
                    .unbind())
            }
        }
    }

    fn time_value(&self, ticks: u32) -> PyResult<Py<PyAny>> {
        if ticks >= 864_000_000 {
            return Err(conversion("Native time out of range"));
        }
        let seconds = ticks / 10_000;
        Ok(self
            .time
            .call1((
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60,
                ticks % 10_000 * 100,
            ))?
            .unbind())
    }

    pub fn row(&self, rows: &Rows, index: usize, as_dict: bool) -> PyResult<Py<PyAny>> {
        let Some(row) = rows.row(index) else {
            return Ok(self.py.None());
        };
        if as_dict {
            let dict = PyDict::new(self.py);
            for (name, value) in rows.names.iter().zip(row) {
                dict.set_item(name, self.value(value)?)?;
            }
            Ok(dict.into_any().unbind())
        } else {
            let values: PyResult<Vec<_>> = row.iter().map(|value| self.value(value)).collect();
            Ok(PyTuple::new(self.py, values?)?.into_any().unbind())
        }
    }

    pub fn all(&self, rows: &Rows, as_dict: bool) -> PyResult<Py<PyAny>> {
        let list = PyList::empty(self.py);
        for index in 0..rows.count {
            list.append(self.row(rows, index, as_dict)?)?;
        }
        Ok(list.into_any().unbind())
    }
}

fn conversion(message: &str) -> PyErr {
    from_core(Error::new(
        ErrorKind::DataConversion,
        "python_conversion",
        message,
    ))
}
fn ordinal(value: &Bound<'_, PyAny>) -> PyResult<i32> {
    Ok(value.call_method0("toordinal")?.extract::<i32>()? - MJD_ORDINAL)
}
fn naive(value: &Bound<'_, PyAny>) -> PyResult<()> {
    if !value.getattr("tzinfo")?.is_none() {
        return Err(conversion("Timezone-aware binding is not implemented yet"));
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
