#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Storage engine abstractions and adapters for HNChain.
//!
//! This crate isolates persistence mechanics from protocol state semantics.
//!
//! [`InMemoryStateStore`] implements `hn_state`'s `StateReader`/
//! `StateWriter` traits (ADR-0019, "Core interfaces") — the first real
//! content this crate has. It is not a durable backend: nothing survives
//! process exit. It exists to prove the trait boundary itself, that
//! protocol code written against `impl StateReader`/`impl StateWriter`
//! (for example `hn_state::active_key`) can run a genuine store-then-
//! query cycle rather than a hand-assembled `Vec<Leaf>`, ahead of
//! ADR-0019's own "initial storage backend" decision (still open, not
//! decided here) about what a real, durable backend looks like.

mod in_memory;

pub use in_memory::InMemoryStateStore;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
