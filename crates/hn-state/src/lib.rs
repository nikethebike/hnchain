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
//! ADR-0007 defines on top of those primitives. Each account section's
//! value schema, once decided in `docs/specs/core/account-state.md`, gets
//! its own module (for example [`envelope_value`]); sections whose value
//! schema is not yet decided are still treated as opaque already-canonical
//! HNCS bytes by callers of this crate.

mod account;
mod envelope_value;
mod error;
mod key;
mod node;
mod tree;

pub use account::{
    AccountSection, DOMAIN_ACCOUNT_EXTENSIONS, DOMAIN_ACCOUNTS, EXTENSION_REGISTRY_ID,
    account_extension_payload_state_key, account_extension_registry_state_key,
    account_section_state_key,
};
pub use envelope_value::{AccountType, ENVELOPE_VERSION_1, EnvelopeValueV1, SectionVersionsV1};
pub use error::{StateError, StateResult};
pub use key::{OBJECT_ID_MAX_LEN, SUBKEY_MAX_LEN, state_key_core, state_key_extension};
pub use node::{EmptyHashTable, TREE_DEPTH, TREE_PROFILE_ID, internal_hash, leaf_hash, value_hash};
pub use tree::{Leaf, compute_state_root};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
