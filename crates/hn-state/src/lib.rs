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
//! (ADR-0006, "Validity Window" / "Access List"). [`receipt`] adds
//! `ReceiptV1` (ADR-0006, "Receipts"); [`transfer`] maps a `transfer`'s
//! outcome onto one via `apply_transfer_with_receipt`. [`list_merkle`]
//! implements `hn-list-merkle-v1` (ADR-0008, "Ordered List
//! Commitment"), the dense ordered-list tree profile `transactions_root`
//! and `receipts_root` use — a separate profile from ADR-0007's sparse
//! `hn-smt-256-v1`, not a reuse of it. [`block_hash`] computes the block
//! hash itself (ADR-0008, "Header Hash") over an already-canonical
//! `BlockHeader` encoding, and [`tx_id`] computes a transaction ID
//! (ADR-0006, "Transaction ID") the same way over an already-canonical
//! `TransactionEnvelope` encoding. [`validator_digest`], [`evidence_digest`],
//! and [`consensus_root`] add the three remaining consensus-track leaf/
//! root digests (ADR-0010, ADR-0015): `consensus_root` and
//! [`validator_set_commitment`] are two names for the same function, not
//! two independently-computed values. [`vote`] adds
//! `VoteSigningPayloadV1`/`ConsensusVote` and `QuorumCertificate`
//! (ADR-0012) — every structural question the latter depended on
//! (aggregation scheme, signer commitment encoding, voting power integer
//! type) is now decided (ADR-0010, ADR-0012), and both
//! `ConsensusVote.signature`/`QuorumCertificate.aggregate_proof` carry a
//! concrete `hn_crypto::SignatureEnvelope` (ADR-0002), not raw bounded
//! bytes. [`validator_record`] adds `ValidatorRecordV1` (ADR-0010,
//! deliberately narrower than the full conceptual struct — see its own
//! documentation), [`validator`] adds the `validators` domain's own
//! key derivation ([`validator_section_state_key`], mirroring
//! [`account_section_state_key`]'s role for the `accounts` domain, ADR-0007
//! domain `0x0006`), and [`active_set`] adds the `ACTIVE_SET(epoch)`
//! derivation function and the live not-jailed overlay check
//! (ADR-0010/ADR-0015), storage-agnostic pure functions taking an
//! already-fetched candidate slice, the same boundary this crate's own
//! charter draws everywhere else.
//!
//! [`state_store`] adds the `StateReader`/`StateWriter` traits (ADR-0019,
//! "Core interfaces") — the "State Access Interface" layer of ADR-0019's
//! own boundary diagram, this crate's job per its charter, not a storage
//! backend (`hn-storage`, which depends on this crate and implements
//! these traits, never the reverse — no dependency cycle). Only these
//! two of ADR-0019's nine named interfaces are covered; the rest
//! (`StateTransaction`, `StateCommitter`, `BlockStore`, `ProofStore`,
//! `SnapshotStore`, `PruningController`, `ArchiveStore`) have no
//! consumer anywhere in this codebase yet and are deliberately not
//! defined ahead of one. [`active_key`]/[`fetch_validator_record`]
//! ([`active_set`]) are the first real consumers: a `validator_id`
//! lookup against any `impl StateReader`, resolving ADR-0002's
//! "Decided: `SignatureEnvelope` concrete field list" `key_reference`
//! (context-derived, not stored) into an actual `KeyDescriptor`. This is
//! the active-set query interface `QuorumCertificate::decode`'s own
//! documentation flags as still missing, now with a real store-backed
//! path behind it, not just an in-memory slice the caller assembled by
//! hand.
//!
//! `ConsensusVote::verify`/`QuorumCertificate::verify_signatures`
//! ([`vote`]) both check real cryptographic signatures against keys
//! resolved via `active_key`, closing a real gap found along the way:
//! a certificate does not preserve each signer's original
//! `vote_metadata`, so verification reconstructs every signer's payload
//! with it forced empty (ADR-0012, "Decided: `vote_metadata` must be
//! empty for any vote eligible to be certified"). Both check only the
//! cryptographic signature — eligibility (`is_eligible_signer`), quorum
//! satisfaction, and whether the supplied active set is actually
//! correct for the referenced epoch stay the caller's job.
//!
//! [`stake_payload`], [`unstake_payload`], and
//! [`validator_update_payload`] add `StakePayloadV1`/`UnstakePayloadV1`/
//! `ValidatorUpdatePayloadV1` (ADR-0006, "Decided:
//! `stake`/`unstake`/`validator_update` payload shapes") — the first
//! discriminated-union payload in this codebase.
//! `ValidatorUpdatePayloadV1.new_consensus_key` cannot use
//! `hn_hncs::write_optional`/`read_optional` for the same reason
//! `QuorumCertificate.aggregate_proof` cannot use `write_list`/
//! `read_list`: decoding a `KeyDescriptor` can fail with a
//! domain-specific error those generic helpers' `HncsResult`-typed
//! closures cannot express, so its presence flag is hand-rolled instead.
//! `encode_key_descriptor`/`decode_key_descriptor`
//! ([`validator_record`]) are shared between `ValidatorRecordV1` and
//! `ValidatorUpdatePayloadV1`, both of which carry a `KeyDescriptor` on
//! the wire the same way.
//!
//! [`validator_transition`] adds `apply_stake`/`apply_unstake`/
//! `apply_validator_update`, mirroring `apply_transfer`'s own split
//! between codec ([`stake_payload`]/[`unstake_payload`]/
//! [`validator_update_payload`]) and state transition: `stake`/
//! `unstake` mutate `bonded_stake` directly via checked arithmetic
//! (never `voting_power`, which only the capping algorithm's own
//! full-candidate-set recomputation may change — still not implemented
//! anywhere in this crate); `validator_update` is a five-way match over
//! `ValidatorOperation` enforcing the lifecycle diagram's own status
//! preconditions (ADR-0010), with `register` the only operation that
//! creates a `ValidatorRecordV1` rather than mutating an existing one.
//!
//! A real durable storage backend (`hn-storage`'s own "initial storage
//! backend" choice, ADR-0019, still open — an in-memory `StateReader`/
//! `StateWriter` implementation proves the trait boundary, not a
//! persistence guarantee) remains out of scope for this crate.

mod access_list;
mod account;
mod active_set;
mod asset_value;
mod balance_value;
mod block_hash;
mod consensus_root;
mod envelope_value;
mod error;
mod evidence_digest;
mod key;
mod lifecycle_value;
mod list_merkle;
mod node;
mod nonce_value;
mod receipt;
mod stake_payload;
mod state_store;
mod transfer;
mod transfer_payload;
mod tree;
mod tx_id;
mod unstake_payload;
mod validator;
mod validator_digest;
mod validator_record;
mod validator_transition;
mod validator_update_payload;
mod validity_window;
mod vote;

pub use access_list::{AccessListV1, MAX_ACCESS_LIST_ENTRIES};
pub use account::{
    AccountSection, DOMAIN_ACCOUNT_EXTENSIONS, DOMAIN_ACCOUNTS, EXTENSION_REGISTRY_ID,
    account_extension_payload_state_key, account_extension_registry_state_key,
    account_section_state_key,
};
pub use active_set::{active_key, active_set, fetch_validator_record, is_eligible_signer};
pub use asset_value::{ASSET_VERSION_1, AssetValueV1, MAX_ASSET_HOLDINGS};
pub use balance_value::{BALANCE_VERSION_1, BalanceValueV1};
pub use block_hash::block_hash;
pub use consensus_root::{consensus_root, validator_set_commitment};
pub use envelope_value::{AccountType, ENVELOPE_VERSION_1, EnvelopeValueV1, SectionVersionsV1};
pub use error::{StateError, StateResult};
pub use evidence_digest::evidence_digest;
pub use key::{OBJECT_ID_MAX_LEN, SUBKEY_MAX_LEN, state_key_core, state_key_extension};
pub use lifecycle_value::{LIFECYCLE_VERSION_1, LifecycleState, LifecycleValueV1};
pub use list_merkle::{LIST_TREE_PROFILE_ID, list_empty_root, list_merkle_root, list_node_hash};
pub use node::{EmptyHashTable, TREE_DEPTH, TREE_PROFILE_ID, internal_hash, leaf_hash, value_hash};
pub use nonce_value::{NONCE_VERSION_1, NonceValueV1};
pub use receipt::{RECEIPT_VERSION_1, ReceiptStatus, ReceiptV1};
pub use stake_payload::{STAKE_PAYLOAD_VERSION_1, StakePayloadV1};
pub use state_store::{StateReader, StateWriter};
pub use transfer::{TransferParty, apply_transfer, apply_transfer_with_receipt};
pub use transfer_payload::{TRANSFER_PAYLOAD_VERSION_1, TransferPayloadV1};
pub use tree::{Leaf, compute_state_root};
pub use tx_id::tx_id;
pub use unstake_payload::{UNSTAKE_PAYLOAD_VERSION_1, UnstakePayloadV1};
pub use validator::{DOMAIN_VALIDATORS, ValidatorSection, validator_section_state_key};
pub use validator_digest::validator_digest;
pub use validator_record::{
    PendingUnbondingV1, RECORD_VERSION_1, ValidatorRecordV1, ValidatorStatus,
};
pub use validator_transition::{
    UNBONDING_PERIOD_BLOCKS, apply_stake, apply_stake_with_receipt, apply_unbonding_release,
    apply_unstake, apply_unstake_with_receipt, apply_validator_update,
    apply_validator_update_with_receipt,
};
pub use validator_update_payload::{
    VALIDATOR_UPDATE_PAYLOAD_VERSION_1, ValidatorOperation, ValidatorUpdatePayloadV1,
};
pub use validity_window::ValidityWindowV1;
pub use vote::{
    CONSENSUS_PROFILE_TENDERMINT_V1, ConsensusVote, MAX_QUORUM_SIGNATURES,
    MAX_SIGNER_COMMITMENT_LEN, MAX_VOTE_METADATA_LEN, QC_VERSION_1, QuorumCertificate,
    VOTE_VERSION_1, VoteSigningPayloadV1, VoteTargetType, VoteType,
};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
