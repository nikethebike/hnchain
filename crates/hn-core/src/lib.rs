#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Core domain primitives for HNChain.
//!
//! This crate contains small, explicit primitive types shared across HNChain
//! modules. It does not define consensus behavior, canonical serialization, or
//! runtime policy by itself.

mod chain;
mod epoch;
mod error;
mod height;
mod length;
mod nonce;
mod round;
mod time;
mod version;

pub use chain::ChainId;
pub use epoch::Epoch;
pub use error::{PrimitiveError, PrimitiveResult};
pub use height::BlockHeight;
pub use length::ByteLength;
pub use nonce::AccountNonce;
pub use round::Round;
pub use time::UnixTimeMillis;
pub use version::ProtocolVersion;
