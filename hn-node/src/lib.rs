#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Node composition layer for HNChain.
//!
//! This crate wires protocol, storage, networking, RPC, configuration, and
//! process lifecycle components through explicit interfaces.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
