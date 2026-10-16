pub mod bind;
pub mod decimal;
mod parameter;
mod plan;
mod value;
pub use fbfast_native::ZonedValue;
pub use parameter::encode_native;
pub use plan::DecodePlan;
pub use value::Value;

/// Worker-owned resources/utilities. Ordinary scalar cells decode without a call.
pub trait NativeReader {
    fn blob(&mut self, id: [u8; 8], charset: u32) -> Result<Vec<u8>, crate::error::Error>;
    fn decimal(&mut self, kind: u32, data: &[u8]) -> Result<String, crate::error::Error>;
    fn zoned(
        &mut self,
        kind: u32,
        data: &[u8],
    ) -> Result<fbfast_native::ZonedValue, crate::error::Error>;
}

struct NoNative;
impl NativeReader for NoNative {
    fn zoned(
        &mut self,
        _: u32,
        _: &[u8],
    ) -> Result<fbfast_native::ZonedValue, crate::error::Error> {
        Err(plan::conversion(
            "Time zone decoding requires the native utility",
        ))
    }
    fn blob(&mut self, _: [u8; 8], _: u32) -> Result<Vec<u8>, crate::error::Error> {
        Err(plan::conversion(
            "BLOB decoding requires an active transaction",
        ))
    }
    fn decimal(&mut self, _: u32, _: &[u8]) -> Result<String, crate::error::Error> {
        Err(plan::conversion(
            "DECFLOAT decoding requires the native utility",
        ))
    }
}
