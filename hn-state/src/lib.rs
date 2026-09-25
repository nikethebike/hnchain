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
//!
//! [`governance_payload`], [`governance`], [`proposal_id`],
//! [`proposal_record`], [`proposal_vote_record`], and
//! [`governance_transition`] add `GovernancePayloadV1` and its state
//! transitions (ADR-0025, "Governance Model"): `propose`/`vote`
//! payload codec, the `governance` domain's key derivation (corrected
//! there from a pure singleton to a singleton-plus-collection shape,
//! ADR-0007), `proposal_id` derivation (mirrors `tx_id`), and
//! `apply_propose`/`apply_vote`/`finalize_proposal`. Signaling-only
//! (ADR-0025, "Decided: Signaling Only") — a `Passed` proposal has no
//! automatic effect anywhere in this crate or elsewhere. Quorum
//! percentage (`GOVERNANCE_QUORUM_NUMERATOR`/`_DENOMINATOR`) and
//! voting-window length (`GOVERNANCE_VOTING_WINDOW`) are now real
//! constants (ADR-0023's own already-decided `1/5`, `302_400` blocks) —
//! `apply_propose`/`finalize_proposal` still take them as parameters
//! rather than reading the constants directly, so both stay testable
//! against other values without redefining the constants themselves,
//! the same `max_size`-style pattern `active_set` already used.
//! [`governance_transition::chamber_weights`] computes
//! [`governance_transition::ChamberWeights`] from an already-fetched
//! candidate slice — the query this module's own documentation used to
//! name as missing, closed the same way `active_set` already resolved
//! the identical "this crate can't enumerate storage" structural
//! problem (ADR-0032, "Governance Chamber-Weight Query And Propose/Vote
//! Wiring"). `fetch_proposal`/`fetch_proposal_vote` are the point-lookup
//! fetch helpers `apply_transaction` needed but nothing had built yet,
//! mirroring `fetch_identity`'s own shape. `finalize_proposal` still has
//! no caller — closing a proposal needs a per-block sweep over every
//! still-`Voting` proposal, which needs enumerating "every open
//! proposal," the same missing capability `chamber_weights` needed a
//! caller-supplied slice to work around; `propose`/`vote` don't have
//! that problem since their target (a proposal, an account) is always a
//! single already-known key.
//!
//! [`key_descriptor`] (crate-private) holds `encode_key_descriptor`/
//! `decode_key_descriptor`, shared by every wire location carrying a
//! `hn_crypto::KeyDescriptor` the same way — `ValidatorRecordV1.
//! consensus_key`/`ValidatorUpdatePayloadV1.new_consensus_key`
//! (`validator_consensus` role) and, new in this pass,
//! `MultisigConfigV1.authorized_keys` (`account_signing` role).
//! [`permission_value`], [`permission_update_payload`], and
//! [`permission_transition`] add `PermissionValueV1`/
//! `PermissionUpdatePayloadV1` and their state transition (ADR-0026,
//! "Threshold And Multisignature Authorization"): activates
//! account-state.md §4.5 Permission State for the `account_signing`
//! role only — 7 of its 8 conceptual capabilities remain unaddressed.
//! `authorized_keys` reuses HNCS's own canonical `set` encoding on the
//! encode side ([`hn_hncs::write_set`]); the decode side hand-rolls the
//! equivalent sort/duplicate check inline, the same
//! `decode_key_descriptor`-can-fail-with-a-domain-error reason
//! `QuorumCertificate.aggregate_proof`/`ValidatorUpdatePayloadV1.
//! new_consensus_key` already couldn't use `read_list`/`read_optional`
//! directly. `verify_multisig_authorization` is the multi-signature
//! verification rule itself — a pure function taking an
//! already-fetched `MultisigConfigV1`, mirroring every other `apply_*`
//! function's "caller already resolved state" boundary.
//!
//! [`identity_transition`]'s `apply_identity_rotation` and a third
//! `PermissionUpdatePayloadV1` shape (ADR-0028, "Account-Level Key
//! Rotation") add single-key-mode rotation: `PermissionUpdatePayloadV1`
//! is `SetAccountSigningMultisig(MultisigConfigV1)` (`payload_version =
//! 1`, ADR-0026, wire-unchanged), `RotateIdentityKey(KeyDescriptor)`
//! (`payload_version = 2`, ADR-0028), or `DeactivateMultisig(KeyDescriptor)`
//! (`payload_version = 3`, ADR-0029, "Multisig Deactivation") —
//! `payload_version` itself is the discriminant for all three, mirroring
//! `SignatureEnvelope`'s own `envelope_version` 1-vs-2 split, so no
//! separate operation byte was added on top of it (`RotateIdentityKey`/
//! `DeactivateMultisig` share one wire shape but stay two distinct
//! versions rather than being told apart by state, the first exception
//! every other `apply_*` function's "payload alone determines the
//! transition" boundary would otherwise have needed). `apply_permission_update`
//! dispatches on the payload variant the same "one entry point per
//! `tx_type`, internal match" shape `apply_validator_update` already
//! uses, and now returns `Vec<Leaf>` rather than a fixed-size array —
//! `DeactivateMultisig` is the first `permission_update` operation that
//! touches two sections (Permission cleared, Identity set to the
//! group-chosen successor) in one transaction, so the other two
//! operations' shared `[Leaf; 1]` shape no longer fits every case.
//! Rotation activates immediately (no epoch-boundary-style delay like
//! ADR-0010's validator case — nonce ordering already serializes
//! same-sender transactions, so there is no concurrency hazard for a
//! delay to guard against) and is rejected while a multisig
//! configuration is active; deactivation is authorized by
//! `verify_multisig_authorization` against the pre-transaction
//! configuration, the same rule an ordinary reconfiguration already
//! uses — `TransactionEnvelope::verify` (below) needed no changes at
//! all to support it, since it already dispatches to multisig
//! verification for any `permission_update` payload whenever
//! `account_signing_multisig` is active.
//!
//! [`transaction_envelope`] adds `TransactionEnvelope`/
//! `TransactionSigningPayload` themselves as concrete Rust types
//! (ADR-0006, every field decision through ADR-0027's `bootstrap_key`)
//! — previously the only major structure in this project that existed
//! solely "by field," each piece ([`stake_payload`], [`access_list`],
//! [`validity_window`], `hn_crypto::SignatureEnvelope`, ...) decided and
//! implemented independently with no containing struct. `payload` stays
//! opaque bytes on `TransactionEnvelope` itself, not a
//! [`TransactionPayload`] field directly — 3 of 9 `tx_type`s
//! (`contract_deploy`/`contract_call`/`system`) have no decided payload
//! schema yet, so decoding the envelope must not require every
//! `tx_type` to be interpretable; [`decode_transaction_payload`]
//! interprets `payload` separately, given `tx_type`, for the 6 that do.
//! [`ValidityWindowV1`]/[`AccessListV1`] gained `encode_into`/
//! `decode_from` alongside their existing standalone `encode`/`decode`,
//! the same flat-embedding convention `SignatureEnvelope` already
//! established, so `TransactionEnvelope` can embed them without a
//! redundant length-prefixed wrapper. [`identity_value`] adds
//! `IdentityValueV1` (`SectionId 0x01`, ADR-0027) — the account's
//! currently-active `account_signing` key, stored as a `KeyDescriptor`
//! the same way `ValidatorRecordV1.consensus_key` already is.
//! [`identity_transition`] adds `fetch_identity` (mirrors
//! `fetch_validator_record`'s own shape for the `accounts` domain),
//! `resolve_account_signing_key` (the first concrete resolution of
//! ADR-0002's `active_key(identity, role, height)` for
//! `account_signing` — pure, no state access itself, enforces
//! ADR-0027's presence rule and address-derivation check),
//! `apply_identity_bootstrap`, and `apply_identity_rotation` (ADR-0028
//! — structurally identical leaf writes, kept as separate
//! intent-revealing names).
//!
//! `TransactionEnvelope::verify(&self, reader: &impl StateReader)`
//! (ADR-0026/ADR-0027/ADR-0028) is the composing call every one of
//! those primitives was built for: fetches `PermissionValueV1`, and
//! dispatches to `verify_multisig_authorization` when
//! `account_signing_multisig` is active or to
//! `resolve_account_signing_key` + an ordinary
//! `SignatureEnvelope::verify` otherwise (enforcing single-key mode's
//! own "exactly one signature, no `key_reference`" rule along the way).
//! Scoped the same way `ConsensusVote::verify` already is —
//! cryptographic authorization only, not `payload` execution or the
//! other envelope fields' own validity — and it never writes: a
//! successful bootstrap still needs a separate
//! `apply_identity_bootstrap` call to actually produce the
//! `IdentityValueV1` leaf, since `verify` only takes a `StateReader`.
//!
//! [`nonce_transition`] adds `fetch_nonce`/`nonce_write` (mirrors
//! `fetch_identity`/`identity_value_write`'s own shape for the Nonce
//! section, absence maps to `AccountNonce::INITIAL`). [`block_transition`]
//! adds `apply_transaction`/`apply_block` (ADR-0030, "Transaction And
//! Block Application") — the composing call this crate's own
//! documentation named as missing at every one of the last several
//! passes ("no block-processing pipeline exists in this codebase yet"):
//! `apply_transaction` calls `TransactionEnvelope::verify`, writes the
//! bootstrap `IdentityValueV1` entry when `bootstrap_key` was present
//! (the separate call `verify`'s own documentation says a caller must
//! make), checks `nonce` against `fetch_nonce` exactly
//! ([`StateError::NonceMismatch`]) and `validity_window` against the
//! caller-supplied `current_height`
//! ([`StateError::TransactionOutsideValidityWindow`]), dispatches
//! `transfer`/`stake`/`unstake`/`validator_update`/`permission_update`/
//! `governance` to their existing `apply_*_with_receipt` functions, and
//! always appends the nonce-update write — ADR-0006's "nonce consumed
//! on inclusion even on failure" rule, implemented here for the first
//! time. Deducts no fee (still-undecided ADR-0023 parameter). Since
//! ADR-0032 ("Governance Chamber-Weight Query And Propose/Vote
//! Wiring"), `governance` is fully wired: `apply_transaction` gained a
//! `validator_candidates: &[ValidatorRecordV1]` parameter (unused by
//! every non-`Propose` payload) so a `Propose` can compute a live
//! `ChamberWeights` snapshot without this crate needing to enumerate
//! storage itself; `apply_block` threads it through unchanged for every
//! transaction in the block, which matches ADR-0025's own
//! height-granularity snapshot discipline rather than falling short of
//! it. `apply_block` applies every transaction in a slice against an
//! [`OverlayReader`] layering each transaction's own writes over the
//! base `reader` before the next transaction runs, then aggregates
//! `tx_id`s/receipt digests into `transactions_root`/`receipts_root` via
//! `list_merkle_root` — it does not compute `BlockHeader.state_root`
//! (needs the complete current leaf set, no durable backend exists yet,
//! ADR-0019).
//!
//! [`state_store::Write`] and [`overlay_reader::OverlayReader`]
//! (ADR-0031, "Write-Set Value Bytes And Overlay State Reader") are why
//! `apply_block` can do that at all: every `apply_*` function in this
//! crate now returns `Write`/`[Write; N]`/`Vec<Write>` — real canonical
//! value bytes, exactly what `StateReader::get` would return for that
//! key after the write applies — rather than the earlier `Leaf`-only
//! `(state_key, leaf_hash)` shape, which was built for
//! `compute_state_root` and could never itself answer a `get` call.
//! [`tree::leaf_for_write`] derives a `Leaf` from a `Write` on demand
//! (used by `compute_state_root` once a caller has a complete leaf set
//! to feed it, not by anything in this crate today); `OverlayReader`
//! wraps a base `StateReader` plus a block's accumulated writes so far,
//! consulted first on every `get`, so a second same-sender transaction
//! in one block now sees the first one's nonce/balance/etc. writes
//! exactly as it would across two separate blocks — the "Intra-block
//! same-sender visibility" gap ADR-0030 named and deferred, now closed.
//!
//! [`state_store::StateCommitter`] (ADR-0033, "Atomic Write-Set
//! Commit") is the interface `StateWriter` itself was missing to be
//! callable safely from real protocol code: `StateWriter::set` is
//! single-key-at-a-time, so a naive loop over a block's write-set could
//! durably apply some entries and not others on a mid-block backend
//! failure — exactly the "partially committed state" ADR-0019's own
//! "Atomic State Commit" rule forbids. `StateCommitter::commit(writes:
//! &[Write])` is all-or-nothing by contract, deliberately narrower than
//! ADR-0019's full atomic-commit scope (block header/body/receipts/
//! events/consensus metadata all still have no concrete stored
//! representation in this codebase). [`block_transition::apply_and_commit_block`]
//! is its first real caller: runs `apply_block` against a store, then
//! commits every transaction's write-set as one unit — the first time
//! anywhere in this codebase that `StateWriter`/`StateCommitter` are
//! reached from actual `apply_transaction` output rather than
//! test-hand-seeded bytes. `apply_block` itself is unchanged and still
//! usable standalone (no commit) for pure simulation/inspection.
//!
//! [`BlockHeader`]/[`BlockBody`] (ADR-0008) assemble every one of those
//! previously-scattered decisions into concrete types for the first
//! time — this pass does not decide anything new, it is the first
//! place `block_hash`, `consensus_root`, `evidence_digest`,
//! `extra_data_hash`, `protocol_parameters_placeholder_hash`,
//! `list_merkle_root`/`list_empty_root`, and `hn_core`'s own
//! `BlockHeight`/`Round`/`Epoch`/`ProtocolEpoch`/`UnixTimeMillis` types
//! are actually assembled together, rather than existing as
//! independent, currently-callerless primitives. `BlockBody`'s own
//! `transactions_root`/`receipts_root`/`evidence_root`/
//! `extra_data_hash` methods compute the matching `BlockHeader` root
//! directly from body content; `events_root`/`consensus_root`/
//! `state_root`/`parent_block_hash`/`proposer` remain values the
//! caller supplies (external validator-set/state-tree/event/genesis
//! data this crate has no way to derive from `BlockBody` alone).
//! `BlockEnvelope` (the `block_version`/`header`/`body`/`justification`
//! wrapper) and genesis's own mapping into a real header are
//! deliberately not attempted here — named as still-open, not silently
//! assumed.

mod access_list;
mod account;
mod active_set;
mod asset_value;
mod balance_value;
mod block_body;
mod block_hash;
mod block_header;
mod block_transition;
mod consensus_root;
mod envelope_value;
mod error;
mod evidence_digest;
mod extra_data;
mod governance;
mod governance_payload;
mod governance_transition;
mod hncoin;
mod identity_transition;
mod identity_value;
mod key;
mod key_descriptor;
mod lifecycle_value;
mod list_merkle;
mod node;
mod nonce_transition;
mod nonce_value;
mod overlay_reader;
mod permission_transition;
mod permission_update_payload;
mod permission_value;
mod proposal_id;
mod proposal_record;
mod proposal_vote_record;
mod protocol_parameters;
mod receipt;
mod stake_payload;
mod state_store;
mod transaction_envelope;
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
pub use block_body::{
    BODY_VERSION_1, BlockBody, MAX_EVIDENCE_BLOB_LEN, MAX_EVIDENCE_PER_BLOCK, MAX_RECEIPT_BLOB_LEN,
    MAX_TRANSACTIONS_PER_BLOCK,
};
pub use block_hash::block_hash;
pub use block_header::{BlockHeader, HEADER_VERSION_1};
pub use block_transition::{
    AppliedTransaction, BlockApplicationResult, apply_and_commit_block, apply_block,
    apply_transaction,
};
pub use consensus_root::{consensus_root, validator_set_commitment};
pub use envelope_value::{AccountType, ENVELOPE_VERSION_1, EnvelopeValueV1, SectionVersionsV1};
pub use error::{StateError, StateResult};
pub use evidence_digest::evidence_digest;
pub use extra_data::{MAX_EXTRA_DATA_LEN, extra_data_hash};
pub use governance::{
    DOMAIN_GOVERNANCE, GovernanceSection, proposal_record_state_key, proposal_vote_record_state_key,
};
pub use governance_payload::{
    GOVERNANCE_PAYLOAD_VERSION_1, GOVERNANCE_PROPOSAL_TITLE_MAX_LEN, GovernanceOperation,
    GovernancePayloadV1, VoteChoice,
};
pub use governance_transition::{
    ChamberWeights, GOVERNANCE_QUORUM_DENOMINATOR, GOVERNANCE_QUORUM_NUMERATOR,
    GOVERNANCE_VOTING_WINDOW, ProposeOutcome, apply_propose, apply_propose_with_receipt,
    apply_vote, apply_vote_with_receipt, chamber_weights, fetch_proposal, fetch_proposal_vote,
    finalize_proposal,
};
pub use hncoin::{
    COMMUNITY_ALLOCATION, FOUNDER_ALLOCATION, GENESIS_SUPPLY, HNCOIN_DECIMALS, MAX_SUPPLY,
    RESERVE_ALLOCATION,
};
pub use identity_transition::{
    apply_identity_bootstrap, apply_identity_rotation, fetch_identity, resolve_account_signing_key,
};
pub use identity_value::{IDENTITY_VERSION_1, IdentityValueV1};
pub use key::{OBJECT_ID_MAX_LEN, SUBKEY_MAX_LEN, state_key_core, state_key_extension};
pub use lifecycle_value::{LIFECYCLE_VERSION_1, LifecycleState, LifecycleValueV1};
pub use list_merkle::{LIST_TREE_PROFILE_ID, list_empty_root, list_merkle_root, list_node_hash};
pub use node::{EmptyHashTable, TREE_DEPTH, TREE_PROFILE_ID, internal_hash, leaf_hash, value_hash};
pub use nonce_transition::{fetch_nonce, nonce_write};
pub use nonce_value::{NONCE_VERSION_1, NonceValueV1};
pub use overlay_reader::OverlayReader;
pub use permission_transition::{
    apply_permission_update, apply_permission_update_with_receipt, verify_multisig_authorization,
};
pub use permission_update_payload::{
    PERMISSION_UPDATE_PAYLOAD_VERSION_1, PERMISSION_UPDATE_PAYLOAD_VERSION_2,
    PermissionUpdatePayloadV1,
};
pub use permission_value::{
    MAX_AUTHORIZED_KEYS, MultisigConfigV1, PERMISSION_VERSION_1, PermissionValueV1,
};
pub use proposal_id::proposal_id;
pub use proposal_record::{PROPOSAL_RECORD_VERSION_1, ProposalRecordV1, ProposalStatus};
pub use proposal_vote_record::{PROPOSAL_VOTE_RECORD_VERSION_1, ProposalVoteRecordV1};
pub use protocol_parameters::protocol_parameters_placeholder_hash;
pub use receipt::{RECEIPT_VERSION_1, ReceiptStatus, ReceiptV1};
pub use stake_payload::{STAKE_PAYLOAD_VERSION_1, StakePayloadV1};
pub use state_store::{StateCommitter, StateReader, StateWriter, Write};
pub use transaction_envelope::{
    MAX_SIGNATURES, MAX_TRANSACTION_SIZE, TX_VERSION_1, TransactionEnvelope, TransactionPayload,
    TransactionSigningPayload, TxType, decode_transaction_payload,
};
pub use transfer::{
    TransferParty, apply_transfer, apply_transfer_with_receipt, fetch_asset, fetch_balance,
    fetch_transfer_party,
};
pub use transfer_payload::{TRANSFER_PAYLOAD_VERSION_1, TransferPayloadV1};
pub use tree::{Leaf, compute_state_root, leaf_for_write};
pub use tx_id::tx_id;
pub use unstake_payload::{UNSTAKE_PAYLOAD_VERSION_1, UnstakePayloadV1};
pub use validator::{DOMAIN_VALIDATORS, ValidatorSection, validator_section_state_key};
pub use validator_digest::validator_digest;
pub use validator_record::{
    PendingUnbondingV1, RECORD_VERSION_1, ValidatorRecordV1, ValidatorStatus,
};
pub use validator_transition::{
    MINIMUM_VALIDATOR_BOND, UNBONDING_PERIOD_BLOCKS, apply_stake, apply_stake_with_receipt,
    apply_unbonding_release, apply_unstake, apply_unstake_with_receipt, apply_validator_update,
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
