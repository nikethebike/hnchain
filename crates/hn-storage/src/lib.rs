#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Storage engine abstractions and adapters for HNChain.
//!
//! This crate isolates persistence mechanics from protocol state semantics.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
