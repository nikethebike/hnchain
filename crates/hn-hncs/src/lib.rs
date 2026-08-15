#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! HNChain canonical serialization implementation.
//!
//! This crate implements a small HNCS primitive codec foundation. It does not
//! define object schemas, transaction formats, hash inputs, or consensus object
//! layouts by itself.

mod decode;
mod encode;
mod error;

pub use decode::Decoder;
pub use encode::{
    write_bool, write_bytes, write_i8, write_i16, write_i32, write_i64, write_i128, write_string,
    write_u8, write_u16, write_u32, write_u64, write_u128,
};
pub use error::{HncsError, HncsResult};
