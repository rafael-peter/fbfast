//! Narrow, safe Rust ownership boundary around the official Firebird OO API.
mod attachment;
mod blob;
mod control;
mod cursor;
mod decimal;
mod ffi;
mod loader;
mod metadata;
mod statement;
mod status;
mod transaction;
mod zoned;

pub use attachment::{AttachOptions, Connection};
pub use control::CancelHandle;
pub use cursor::RawBatch;
pub use loader::{Client, ClientInfo};
pub use metadata::{Column, Metadata};
pub use statement::StatementId;
pub use status::{DatabaseErrorKind, NativeError};
pub use zoned::{ZonedValue, ZONE_CAPACITY};

#[cfg(feature = "fuzzing")]
pub use metadata::fuzz_metadata;
#[cfg(feature = "fuzzing")]
pub use status::fuzz_status;
