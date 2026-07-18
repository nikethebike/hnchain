#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Account state and state transition boundaries for HNChain.
//!
//! This crate owns protocol state interfaces without binding them to a concrete
//! storage engine.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
