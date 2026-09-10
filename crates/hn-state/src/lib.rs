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
//! its own module (for example [`envelope_value`], [`nonce_value`]);
//! sections whose value schema is not yet decided are still treated as
//! opaque already-canonical HNCS bytes by callers of this crate.
//! [`transfer_payload`] and [`transfer`] add ADR-0006's `transfer`
//! transaction payload and its state transition: decoding a payload is
//! ADR-0006's concern, applying it to produce updated write-set leaves
//! is this crate's, per its own "state transition boundaries" scope.
//! [`validity_window`] and [`access_list`] add the two other decided
//! `TransactionEnvelope` field shapes with their own dedicated encoding
//! (ADR-0006, "Validity Window" / "Access List").

mod access_list;
mod account;
mod asset_value;
mod balance_value;
mod envelope_value;
mod error;
mod key;
mod lifecycle_value;
mod node;
mod nonce_value;
mod transfer;
mod transfer_payload;
mod tree;
mod validity_window;

pub use access_list::{AccessListV1, MAX_ACCESS_LIST_ENTRIES};
pub use account::{
    AccountSection, DOMAIN_ACCOUNT_EXTENSIONS, DOMAIN_ACCOUNTS, EXTENSION_REGISTRY_ID,
    account_extension_payload_state_key, account_extension_registry_state_key,
    account_section_state_key,
};
pub use asset_value::{ASSET_VERSION_1, AssetValueV1, MAX_ASSET_HOLDINGS};
pub use balance_value::{BALANCE_VERSION_1, BalanceValueV1};
pub use envelope_value::{AccountType, ENVELOPE_VERSION_1, EnvelopeValueV1, SectionVersionsV1};
pub use error::{StateError, StateResult};
pub use key::{OBJECT_ID_MAX_LEN, SUBKEY_MAX_LEN, state_key_core, state_key_extension};
pub use lifecycle_value::{LIFECYCLE_VERSION_1, LifecycleState, LifecycleValueV1};
pub use node::{EmptyHashTable, TREE_DEPTH, TREE_PROFILE_ID, internal_hash, leaf_hash, value_hash};
pub use nonce_value::{NONCE_VERSION_1, NonceValueV1};
pub use transfer::{TransferParty, apply_transfer};
pub use transfer_payload::{TRANSFER_PAYLOAD_VERSION_1, TransferPayloadV1};
pub use tree::{Leaf, compute_state_root};
pub use validity_window::ValidityWindowV1;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
