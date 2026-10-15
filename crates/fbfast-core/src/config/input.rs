use crate::error::Error;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

pub type Inputs = BTreeMap<String, Value>;

#[derive(Debug, Default)]
pub enum EnvFile {
    #[default]
    Default,
    Disabled,
    Path(PathBuf),
}

pub(crate) struct Reader(pub Inputs);

impl Reader {
    pub fn text(&mut self, field: &str, default: Option<&str>) -> Result<String, Error> {
        let value = self
            .0
            .remove(field)
            .or_else(|| default.map(|s| Value::String(s.into())));
        match value {
            Some(Value::String(s)) if !s.is_empty() && !s.contains('\0') => Ok(s),
            _ => Err(Error::config(
                field,
                "requires a nonempty string without NUL",
            )),
        }
    }

    pub fn optional_text(&mut self, field: &str) -> Result<Option<String>, Error> {
        match self.0.remove(field) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if s.is_empty() || s.eq_ignore_ascii_case("none") => Ok(None),
            Some(Value::String(s)) if !s.contains('\0') => Ok(Some(s)),
            _ => Err(Error::config(field, "requires text or None")),
        }
    }

    pub fn boolean(&mut self, field: &str, default: bool) -> Result<bool, Error> {
        match self.0.remove(field) {
            None => Ok(default),
            Some(Value::Bool(b)) => Ok(b),
            Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => Ok(true),
                "false" | "0" | "no" => Ok(false),
                _ => Err(Error::config(field, "requires true/false, 1/0 or yes/no")),
            },
            _ => Err(Error::config(field, "requires a boolean")),
        }
    }

    pub fn integer(
        &mut self,
        field: &str,
        default: u64,
        minimum: u64,
        maximum: u64,
    ) -> Result<u64, Error> {
        let parsed = match self.0.remove(field) {
            None => Some(default),
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(s)) => s.parse::<u64>().ok(),
            _ => None,
        };
        parsed
            .filter(|n| *n >= minimum && *n <= maximum)
            .ok_or_else(|| {
                Error::config(
                    field,
                    &format!("requires an integer in {minimum}..={maximum}"),
                )
            })
    }

    pub fn optional_integer(
        &mut self,
        field: &str,
        default: Option<u64>,
    ) -> Result<Option<u64>, Error> {
        if self.is_none(field) {
            self.0.remove(field);
            return Ok(None);
        }
        if !self.0.contains_key(field) {
            return Ok(default);
        }
        self.integer(field, 1, 1, i64::MAX as u64).map(Some)
    }

    pub fn seconds(&mut self, field: &str, default: f64) -> Result<f64, Error> {
        let value = match self.0.remove(field) {
            None => Some(default),
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.parse::<f64>().ok(),
            _ => None,
        };
        // All public durations must fit checked native millisecond timeouts.
        value
            .filter(|n| n.is_finite() && *n > 0.0 && *n * 1000.0 <= u32::MAX as f64)
            .ok_or_else(|| {
                Error::config(
                    field,
                    "requires finite positive seconds fitting a u32 millisecond timeout",
                )
            })
    }

    pub fn optional_seconds(&mut self, field: &str, default: f64) -> Result<Option<f64>, Error> {
        if self.is_none(field) {
            self.0.remove(field);
            return Ok(None);
        }
        self.seconds(field, default).map(Some)
    }

    fn is_none(&self, field: &str) -> bool {
        match self.0.get(field) {
            Some(Value::Null) => true,
            Some(Value::String(s)) => s.eq_ignore_ascii_case("none"),
            _ => false,
        }
    }

    pub fn finish(self) -> Result<(), Error> {
        if let Some(field) = self.0.keys().next() {
            return Err(Error::config(field, "unknown option"));
        }
        Ok(())
    }
}
