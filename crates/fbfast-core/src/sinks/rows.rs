use crate::{
    config::Config,
    decode::{DecodePlan, Value},
    error::{Error, ErrorKind},
};

/// Row-only storage: flattened cells avoid one heap vector allocation per row.
#[derive(Debug)]
pub struct Rows {
    pub names: Vec<String>,
    pub cells: Vec<Value>,
    pub count: usize,
    pub bytes: usize,
}

impl Rows {
    pub fn new(plan: &DecodePlan) -> Self {
        Self {
            names: plan.schema.columns.iter().map(|c| c.name.clone()).collect(),
            cells: Vec::new(),
            count: 0,
            bytes: 0,
        }
    }

    pub fn row(&self, index: usize) -> Option<&[Value]> {
        if index >= self.count {
            return None;
        }
        self.cells
            .get(index * self.names.len()..(index + 1) * self.names.len())
    }

    pub fn push(
        &mut self,
        plan: &DecodePlan,
        message: &[u8],
        config: &Config,
    ) -> Result<(), Error> {
        if config
            .max_result_rows
            .is_some_and(|limit| self.count as u64 >= limit)
        {
            return Err(limit());
        }
        for index in 0..self.names.len() {
            let value = plan.value(index, message)?;
            self.bytes = self
                .bytes
                .checked_add(value.payload_bytes())
                .ok_or_else(limit)?;
            if config
                .max_result_bytes
                .is_some_and(|limit| self.bytes as u64 > limit)
            {
                return Err(limit());
            }
            self.cells.push(value);
        }
        self.count += 1;
        Ok(())
    }

    pub fn require_unique_names(&self) -> Result<(), Error> {
        let mut seen = std::collections::HashSet::new();
        if self.names.iter().any(|name| !seen.insert(name)) {
            return Err(Error::new(
                ErrorKind::DataConversion,
                "describe",
                "Duplicate column names; provide unique aliases",
            ));
        }
        Ok(())
    }
}

fn limit() -> Error {
    Error::new(
        ErrorKind::ResultLimit,
        "materialize",
        "Materialized result exceeds configured rows/bytes",
    )
}
