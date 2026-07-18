#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Cryptographic identity and primitive wrappers for HNChain.
//!
//! This crate must wrap reviewed external cryptographic implementations behind
//! HNChain-owned types. It must not define custom cryptographic algorithms.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
