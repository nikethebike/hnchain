#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Account state and state transition boundaries for HNChain.
//!
//! This crate owns protocol state interfaces without binding them to a concrete
//! storage engine.
//!
//! It implements the `hn-smt-256-v1` state tree profile defined by
//! ADR-0007: state key derivation ([`key`]), tree node hashing
//! ([`node`]), and state root computation ([`tree`]). [`account`] adds
//! the `accounts` and `account_extensions` domain-specific key derivation
//! ADR-0007 defines on top of those primitives. Account and extension
//! value schemas are owned by the account state specification, not this
//! crate; values are treated here as opaque already-canonical HNCS bytes.

mod account;
mod error;
mod key;
mod node;
mod tree;

pub use account::{
    AccountSection, DOMAIN_ACCOUNT_EXTENSIONS, DOMAIN_ACCOUNTS, EXTENSION_REGISTRY_ID,
    account_extension_payload_state_key, account_extension_registry_state_key,
    account_section_state_key,
};
pub use error::{StateError, StateResult};
pub use key::{OBJECT_ID_MAX_LEN, SUBKEY_MAX_LEN, state_key_core, state_key_extension};
pub use node::{EmptyHashTable, TREE_DEPTH, TREE_PROFILE_ID, internal_hash, leaf_hash, value_hash};
pub use tree::{Leaf, compute_state_root};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
