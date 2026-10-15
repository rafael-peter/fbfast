/// Exact native values for the row sink. Analytical/JSON sinks will use the
/// same decode plan directly, without building Python rows or a universal table.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Decimal { coefficient: i128, scale: i32 },
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    Date(i32),
    Time(u32),
    Timestamp { date: i32, time: u32 },
}

impl Value {
    pub fn payload_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Text(value) => value.len(),
                Self::Bytes(value) => value.len(),
                _ => 0,
            }
    }
}
