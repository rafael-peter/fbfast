//! Python objects exist only at the binding boundary, from owned native batches.
use crate::errors::from_core;
use fbfast_core::pool::DeliveryBudget;
use fbfast_core::{
    decode::Value,
    error::{Error, ErrorKind},
    sinks::rows::Rows,
};
use pyo3::{
    prelude::*,
    sync::PyOnceLock,
    types::{PyBytes, PyDate, PyDateTime, PyDict, PyList, PyString, PyTime, PyTuple},
    IntoPyObjectExt,
};
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::{Duration, Instant},
};

const MJD_ORDINAL: i32 = 678576;

/// Modules imported once per process. Attributes are still looked up per
/// result, so replacing e.g. `decimal.Decimal` at runtime keeps working.
struct Modules {
    decimal: Py<PyModule>,
    datetime: Py<PyModule>,
    temporal: Py<PyModule>,
}

static MODULES: PyOnceLock<Modules> = PyOnceLock::new();

fn modules(py: Python<'_>) -> PyResult<&'static Modules> {
    MODULES.get_or_try_init(py, || {
        Ok(Modules {
            decimal: py.import("decimal")?.unbind(),
            datetime: py.import("datetime")?.unbind(),
            temporal: py.import("fbfast.temporal")?.unbind(),
        })
    })
}

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
    zoned_time: Bound<'py, PyAny>,
    timestamp_input: Bound<'py, PyAny>,
    timestamp_output: Bound<'py, PyAny>,
    /// Reused per row so building a tuple allocates no intermediate Vec.
    cells: RefCell<Vec<Py<PyAny>>>,
}

impl<'py> Converter<'py> {
    pub fn new(py: Python<'py>) -> PyResult<Self> {
        let modules = modules(py)?;
        let datetime = modules.datetime.bind(py);
        let temporal = modules.temporal.bind(py);
        Ok(Self {
            budget: None,
            converted: Cell::new(0),
            last_checkpoint: Cell::new(Instant::now()),
            async_control: None,
            py,
            decimal: modules.decimal.bind(py).getattr("Decimal")?,
            date: datetime.getattr("date")?,
            time: datetime.getattr("time")?,
            datetime: datetime.getattr("datetime")?,
            zoned_time: temporal.getattr("TimeWithZone")?,
            timestamp_input: temporal.getattr("_timestamp_parameter")?,
            timestamp_output: temporal.getattr("_timestamp_from_native")?,
            cells: RefCell::new(Vec::new()),
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

    /// Counts converted cells and runs the cancellation/deadline checkpoint
    /// every 512 cells (and every 2 ms in async delivery).
    fn account(&self, cells: usize) -> PyResult<()> {
        let before = self.converted.get();
        let after = before.wrapping_add(cells.max(1));
        self.converted.set(after);
        if before / 512 != after / 512
            || before == 0
            || (self.async_control.is_some()
                && self.last_checkpoint.get().elapsed() >= Duration::from_millis(2))
        {
            self.checkpoint()?;
        }
        Ok(())
    }

    pub fn value(&self, value: &Value) -> PyResult<Py<PyAny>> {
        self.account(1)?;
        self.cell(value)
    }

    fn cell(&self, value: &Value) -> PyResult<Py<PyAny>> {
        let py = self.py;
        match value {
            Value::Null => Ok(py.None()),
            Value::Bool(value) => value.into_py_any(py),
            // Nearly every integer fits i64, whose conversion avoids the
            // byte-array path Python uses for 128-bit values.
            Value::Int(value) => match i64::try_from(*value) {
                Ok(small) => small.into_py_any(py),
                Err(_) => value.into_py_any(py),
            },
            Value::Float(value) => value.into_py_any(py),
            Value::Text(value) => value.into_py_any(py),
            Value::DecFloat(value) => Ok(self.decimal.call1((value,))?.unbind()),
            Value::Bytes(value) if value.len() >= 1024 * 1024 => self.bytes(value),
            Value::Bytes(value) => Ok(PyBytes::new(py, value).into_any().unbind()),
            Value::Decimal { coefficient, scale } => Ok(self
                .decimal
                .call1((decimal_text(*coefficient, *scale),))?
                .unbind()),
            Value::Date(value) => {
                let (year, month, day) =
                    civil(*value).ok_or_else(|| conversion("Date out of range"))?;
                Ok(PyDate::new(py, year, month, day)?.into_any().unbind())
            }
            Value::Time(value) => self.time_value(*value),
            Value::TimeTz { utc_time, zone } => Ok(self
                .zoned_time
                .call1((self.time_value(*utc_time)?, zone))?
                .unbind()),
            Value::TimestampTz(value) => Ok(self
                .timestamp_output
                .call1((
                    value.utc_date,
                    value.utc_time,
                    value.local_date,
                    value.local_time,
                    &value.zone,
                ))?
                .unbind()),
            Value::Timestamp { date, time } => {
                let (year, month, day) =
                    civil(*date).ok_or_else(|| conversion("Date out of range"))?;
                let (hour, minute, second, micro) =
                    clock(*time).ok_or_else(|| conversion("Native time out of range"))?;
                Ok(
                    PyDateTime::new(py, year, month, day, hour, minute, second, micro, None)?
                        .into_any()
                        .unbind(),
                )
            }
        }
    }

    fn time_value(&self, ticks: u32) -> PyResult<Py<PyAny>> {
        let (hour, minute, second, micro) =
            clock(ticks).ok_or_else(|| conversion("Native time out of range"))?;
        Ok(PyTime::new(self.py, hour, minute, second, micro, None)?
            .into_any()
            .unbind())
    }

    pub fn row(&self, rows: &Rows, index: usize, as_dict: bool) -> PyResult<Py<PyAny>> {
        if as_dict {
            let keys = self.keys(rows);
            return self.row_with(rows, index, Some(&keys));
        }
        self.row_with(rows, index, None)
    }

    /// Column names as Python strings, created once per result.
    fn keys(&self, rows: &Rows) -> Vec<Bound<'py, PyString>> {
        rows.names
            .iter()
            .map(|name| PyString::intern(self.py, name))
            .collect()
    }

    fn row_with(
        &self,
        rows: &Rows,
        index: usize,
        keys: Option<&[Bound<'py, PyString>]>,
    ) -> PyResult<Py<PyAny>> {
        let Some(row) = rows.row(index) else {
            return Ok(self.py.None());
        };
        self.account(row.len())?;
        if let Some(keys) = keys {
            let dict = PyDict::new(self.py);
            for (name, value) in keys.iter().zip(row) {
                dict.set_item(name, self.cell(value)?)?;
            }
            return Ok(dict.into_any().unbind());
        }
        let mut cells = self.cells.borrow_mut();
        cells.clear();
        for value in row {
            cells.push(self.cell(value)?);
        }
        Ok(PyTuple::new(self.py, cells.drain(..))?.into_any().unbind())
    }

    pub fn all(&self, rows: &Rows, as_dict: bool) -> PyResult<Py<PyAny>> {
        let keys = as_dict.then(|| self.keys(rows));
        let list = PyList::empty(self.py);
        for index in 0..rows.count {
            list.append(self.row_with(rows, index, keys.as_deref())?)?;
        }
        Ok(list.into_any().unbind())
    }
    pub fn bytes(&self, data: &[u8]) -> PyResult<Py<PyAny>> {
        self.checkpoint()?;
        let bytes = PyBytes::new_with(self.py, data.len(), |destination| {
            for (target, source) in destination
                .chunks_mut(1024 * 1024)
                .zip(data.chunks(1024 * 1024))
            {
                // Unpublished bytes storage; PyO3 owns allocation and initialization.
                self.py.detach(|| target.copy_from_slice(source));
                self.checkpoint()?;
            }
            Ok(())
        })?;
        self.checkpoint()?;
        Ok(bytes.into_any().unbind())
    }
}

fn conversion(message: &str) -> PyErr {
    from_core(Error::new(
        ErrorKind::DataConversion,
        "python_conversion",
        message,
    ))
}

#[path = "values_calendar.rs"]
mod calendar;
#[path = "values_params.rs"]
mod params;
use calendar::{civil, clock, decimal_text};
