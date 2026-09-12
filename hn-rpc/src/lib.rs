#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! RPC surface boundaries for HNChain.
//!
//! This crate exposes versioned API boundaries without bypassing protocol
//! validation layers.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
