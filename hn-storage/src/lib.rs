#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Storage engine abstractions and adapters for HNChain.
//!
//! This crate isolates persistence mechanics from protocol state semantics.
//!
//! [`InMemoryStateStore`] implements `hn_state`'s `StateReader`/
//! `StateWriter` traits (ADR-0019, "Core interfaces") — the first real
//! content this crate had. It is not a durable backend: nothing survives
//! process exit. It exists to prove the trait boundary itself, that
//! protocol code written against `impl StateReader`/`impl StateWriter`
//! (for example `hn_state::active_key`) can run a genuine store-then-
//! query cycle rather than a hand-assembled `Vec<Leaf>`.
//!
//! [`RedbStateStore`] adds the real thing (ADR-0019, "Decided: initial
//! storage backend" — `redb`): a durable, `redb`-backed implementation of
//! the same two traits, one `redb` transaction per `get`/`set` call.
//! Choosing a real backend also meant `StateReader`/`StateWriter`
//! themselves had to become fallible (`hn_state::StateResult`-wrapped,
//! not the bare `Option`/`()` they carried before) — a durable backend
//! can genuinely fail with an I/O error in a way an in-memory `BTreeMap`
//! cannot, and `hn-state`'s own workspace lints forbid papering over that
//! with `.unwrap()`/`.expect()`/`panic!` (`clippy::unwrap_used`,
//! `clippy::expect_used`, `clippy::panic` are all `deny`). Every `redb`
//! error converts to `hn_state::StateError::Storage`'s opaque `String` at
//! this crate's boundary — never a `redb`-typed error escaping into
//! `hn-state` or its callers — matching ADR-0019's own "Backend
//! Independence" rule that changing backends must not change anything
//! observable above the storage interface.

mod in_memory;
mod redb_store;

pub use in_memory::InMemoryStateStore;
pub use redb_store::RedbStateStore;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
