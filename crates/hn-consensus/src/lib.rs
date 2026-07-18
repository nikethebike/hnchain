#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Consensus state machine boundaries for HNChain.
//!
//! This crate must not perform network I/O directly and must not rely on node
//! process lifecycle behavior.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
