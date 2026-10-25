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
//!
//! Both types now also implement `hn_state::StateCommitter` (ADR-0033,
//! "Atomic Write-Set Commit"): `InMemoryStateStore`'s is a plain loop
//! (a `BTreeMap` insert cannot itself fail); `RedbStateStore`'s opens
//! exactly one `redb` transaction for a whole write-set rather than one
//! per key, so a failure partway through leaves the database completely
//! unchanged rather than partially written — the real fix
//! `StateWriter::set`'s own single-key-at-a-time scope could never
//! provide, and the reason nothing in this codebase called `StateWriter`
//! from real protocol output until now
//! (`hn-state::apply_and_commit_block` is the first caller).
//!
//! [`write_atomically`]/[`read_optional`] (ADR-0048, "Decided: Vote
//! Journal (Write-Ahead Safety State)", rule 1) are a separate, plain
//! bytes-in/bytes-out durability primitive — temp file, `fsync`,
//! rename, directory `fsync` — for a small single-file record that
//! lives outside the block/state store entirely (the vote journal is
//! the first caller; nothing here is journal-specific or depends on
//! `hn_state`'s traits).

mod atomic_file;
mod in_memory;
mod redb_store;
mod tx_index;

pub use atomic_file::{read_optional, write_atomically};
pub use in_memory::InMemoryStateStore;
pub use redb_store::RedbStateStore;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
