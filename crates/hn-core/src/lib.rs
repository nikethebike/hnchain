#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Core domain primitives for HNChain.
//!
//! This crate is intentionally limited to the public crate boundary defined by
//! ADR-0021. Protocol behavior is added only after accepted specifications and
//! conformance tests exist.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
