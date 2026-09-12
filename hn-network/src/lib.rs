#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! P2P networking boundaries for HNChain.
//!
//! This crate owns transport-facing abstractions and must not define consensus
//! validity.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
