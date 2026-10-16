//! Per-column builders shared by materialized and streaming Arrow execution.
mod column;
mod schema;
use crate::{
    config::Config,
    decode::{DecodePlan, NativeReader},
    error::{Error, ErrorKind},
};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use column::ColumnBuilder;

#[derive(Debug, Clone, Copy, Default)]
pub enum ArrowMode {
    #[default]
    Native,
    Polars,
    PolarsString,
}

#[derive(Debug)]
pub struct ArrowOutput {
    pub schema: SchemaRef,
    pub batches: Vec<RecordBatch>,
    pub rows: usize,
    pub bytes: usize,
}

pub struct ArrowBuilder {
    pub schema: SchemaRef,
    columns: Vec<ColumnBuilder>,
    pub rows: usize,
}
impl ArrowBuilder {
    pub fn new(
        plan: &DecodePlan,
        config: &Config,
        mode: ArrowMode,
        rows: usize,
    ) -> Result<Self, Error> {
        let schema = schema::schema(plan, config, mode)?;
        Self::with_schema(schema, rows, config.batch_bytes as usize)
    }
    pub fn with_schema(schema: SchemaRef, rows: usize, bytes: usize) -> Result<Self, Error> {
        let column_bytes = bytes / schema.fields().len().max(1);
        let columns = schema
            .fields()
            .iter()
            .map(|field| ColumnBuilder::new(field.data_type(), rows, column_bytes))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            schema,
            columns,
            rows: 0,
        })
    }
    pub fn push(
        &mut self,
        plan: &DecodePlan,
        row: &[u8],
        native: &mut impl NativeReader,
    ) -> Result<(), Error> {
        for (index, column) in self.columns.iter_mut().enumerate() {
            let value = plan.value_with_native(index, row, native)?;
            column.append(value)?;
        }
        self.rows += 1;
        Ok(())
    }
    pub fn finish(&mut self) -> Result<RecordBatch, Error> {
        let columns = self.columns.iter_mut().map(ColumnBuilder::finish).collect();
        let batch = RecordBatch::try_new(self.schema.clone(), columns)
            .map_err(|_| conversion("Invalid Arrow record batch"))?;
        self.rows = 0;
        Ok(batch)
    }
}

pub(super) fn conversion(message: &str) -> Error {
    Error::new(ErrorKind::DataConversion, "arrow", message)
}
