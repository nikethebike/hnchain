#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Command-line interface boundary for HNChain.
//!
//! This crate is a client of explicit node and RPC interfaces, not a private
//! protocol-state backdoor.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
