/// Exact native values for the row sink. Analytical/JSON sinks will use the
/// same decode plan directly, without building Python rows or a universal table.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Decimal { coefficient: i128, scale: i32 },
    DecFloat(String),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    Date(i32),
    Time(u32),
    Timestamp { date: i32, time: u32 },
    TimeTz { utc_time: u32, zone: String },
    TimestampTz(Box<fbfast_native::ZonedValue>),
}

impl Value {
    pub fn payload_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Text(value) | Self::DecFloat(value) => value.len(),
                Self::Bytes(value) => value.len(),
                Self::TimeTz { zone, .. } => zone.len(),
                Self::TimestampTz(value) => {
                    std::mem::size_of::<fbfast_native::ZonedValue>() + value.zone.len()
                }
                _ => 0,
            }
    }
}
