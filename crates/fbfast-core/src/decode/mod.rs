pub mod bind;
mod plan;
mod value;
pub use plan::DecodePlan;
pub use value::Value;

/// Worker-owned source; only BLOB cells cross this boundary, never scalar cells.
pub trait BlobReader {
    fn read(&mut self, id: [u8; 8], charset: u32) -> Result<Vec<u8>, crate::error::Error>;
}

struct NoBlobs;
impl BlobReader for NoBlobs {
    fn read(&mut self, _: [u8; 8], _: u32) -> Result<Vec<u8>, crate::error::Error> {
        Err(plan::conversion(
            "BLOB decoding requires an active transaction",
        ))
    }
}
