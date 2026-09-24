use hn_crypto::{Digest, HashError, IdentityError};
use hn_hncs::HncsError;

/// Result type used by state key derivation and state tree operations.
pub type StateResult<T> = Result<T, StateError>;

/// Errors produced while deriving state keys, hashing tree nodes, or
/// computing a state root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateError {
    /// A hash profile input could not be constructed or hashed. This also
    /// carries HNCS framing failures for `object_id` / `subkey`, which are
    /// folded into [`HashError::Framing`] on the way in.
    Hash(HashError),
    /// Two leaves in the same write set share the same `state_key`
    /// (ADR-0007: the state tree layer accepts a deterministic final write
    /// set; it does not resolve write-set conflicts).
    DuplicateStateKey,
    /// `extension_id = 0x0000` was used where an extension payload leaf
    /// was expected; that value is reserved for the extension registry
    /// leaf (ADR-0007, Account Extensions Domain: Registry And Payload
    /// Leaves).
    ReservedExtensionId,
    /// A value schema (for example `EnvelopeValueV1`) failed to encode
    /// or decode as canonical HNCS. Distinct from [`StateError::Hash`],
    /// which is scoped to `object_id` / `subkey` framing during key
    /// derivation.
    Encoding(HncsError),
    /// A decoded `EnvelopeValueV1.account_type` byte is not a member of
    /// the `account_type` registry (account-state.md §3.1). `0x00` is
    /// reserved and always invalid.
    InvalidAccountType {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `EnvelopeValueV1.envelope_version` does not match
    /// [`crate::envelope_value::ENVELOPE_VERSION_1`], the only shape this
    /// implementation understands.
    UnsupportedEnvelopeVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `NonceValueV1.nonce_version` does not match
    /// [`crate::nonce_value::NONCE_VERSION_1`], the only shape this
    /// implementation understands.
    UnsupportedNonceVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `BalanceValueV1.balance_version` does not match
    /// [`crate::balance_value::BALANCE_VERSION_1`], the only shape this
    /// implementation understands.
    UnsupportedBalanceVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `AssetValueV1.asset_version` does not match
    /// [`crate::asset_value::ASSET_VERSION_1`], the only shape this
    /// implementation understands.
    UnsupportedAssetVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `LifecycleValueV1.lifecycle_version` does not match
    /// [`crate::lifecycle_value::LIFECYCLE_VERSION_1`], the only shape
    /// this implementation understands.
    UnsupportedLifecycleVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `LifecycleValueV1.state` byte is not a member of the
    /// closed `state` registry (account-state.md §4.9).
    InvalidLifecycleState {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `TransferPayloadV1.payload_version` does not match
    /// [`crate::transfer_payload::TRANSFER_PAYLOAD_VERSION_1`], the only
    /// shape this implementation understands.
    UnsupportedTransferPayloadVersion {
        /// The rejected version.
        value: u16,
    },
    /// A `transfer` (ADR-0006, "Payload") would debit more than the
    /// sender's applicable balance — the validation precondition
    /// "the sender's applicable balance must be at least `amount`"
    /// failed.
    InsufficientBalance,
    /// Applying a `transfer`'s credit side would overflow the
    /// recipient's applicable balance's `u128` range.
    BalanceOverflow,
    /// A decoded `ReceiptV1.receipt_version` does not match
    /// [`crate::receipt::RECEIPT_VERSION_1`], the only shape this
    /// implementation understands.
    UnsupportedReceiptVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `ReceiptV1.status` byte is not a member of the closed
    /// `status` registry (ADR-0006, "Receipts").
    InvalidReceiptStatus {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `VoteSigningPayloadV1`/`ConsensusVote.vote_version`
    /// does not match [`crate::vote::VOTE_VERSION_1`], the only shape
    /// this implementation understands.
    UnsupportedVoteVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `consensus_profile` does not match
    /// [`crate::vote::CONSENSUS_PROFILE_TENDERMINT_V1`], the only
    /// profile this implementation understands.
    UnsupportedConsensusProfile {
        /// The rejected profile identifier.
        value: u16,
    },
    /// A decoded `vote_type` byte is not a member of the closed
    /// registry (ADR-0012, "Decided: `vote_type` registry").
    InvalidVoteType {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `target_type` byte is not a member of the closed
    /// registry (ADR-0012, "Decided: `target_type` registry").
    InvalidVoteTargetType {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded vote has `target_type = Nil` but a non-zero
    /// `target_hash` — the only canonical encoding of a nil vote has
    /// an all-zero `target_hash` (ADR-0012, "Decided: `target_type`
    /// registry").
    NonCanonicalNilTarget,
    /// A decoded `QuorumCertificate.qc_version` does not match
    /// [`crate::vote::QC_VERSION_1`], the only shape this
    /// implementation understands.
    UnsupportedQcVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `QuorumCertificate`'s `aggregate_proof` entry count
    /// does not match `signer_commitment`'s set-bit count — the two
    /// must name the same signers in the same order (ADR-0012,
    /// "Decided: individual signatures with bitmap").
    SignerCountMismatch,
    /// A decoded `QuorumCertificate` has `signed_voting_power >
    /// total_voting_power`, which can never be a valid certificate
    /// regardless of the active validator set.
    SignedVotingPowerExceedsTotal,
    /// A decoded `ValidatorRecordV1.record_version` does not match
    /// [`crate::validator_record::RECORD_VERSION_1`], the only shape
    /// this implementation understands.
    UnsupportedRecordVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded [`hn_crypto::KeyDescriptor`]'s `algorithm_id` is not
    /// Ed25519 (`hn_crypto::ED25519_ALGORITHM_ID`), the only active
    /// signing suite at genesis (ADR-0002, "Accepted Initial
    /// Direction") — for `ValidatorRecordV1.consensus_key`/
    /// `ValidatorUpdatePayloadV1.new_consensus_key` (`validator_consensus`
    /// role) or `MultisigConfigV1.authorized_keys` (`account_signing`
    /// role, ADR-0026); [`crate::key_descriptor::decode_key_descriptor`]
    /// is shared by both.
    UnsupportedKeyAlgorithm {
        /// The rejected algorithm identifier.
        value: u16,
    },
    /// A decoded [`hn_crypto::KeyDescriptor`]'s public key bytes were
    /// rejected by [`hn_crypto::KeyDescriptor`] itself — wrong length
    /// for the declared algorithm, or not a canonical Ed25519 point.
    /// Shared across every `key_descriptor`-decoding site, the same as
    /// [`StateError::UnsupportedKeyAlgorithm`].
    InvalidConsensusKey(IdentityError),
    /// A decoded `ValidatorRecordV1.status` byte is not a member of the
    /// closed `status` registry (ADR-0010, "Validator Status").
    InvalidValidatorStatus {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `ConsensusVote.signature` / `QuorumCertificate.
    /// aggregate_proof` entry was rejected as a
    /// [`hn_crypto::SignatureEnvelope`] — unsupported `envelope_version`,
    /// most commonly.
    InvalidSignatureEnvelope(IdentityError),
    /// No [`crate::ValidatorRecordV1`] is stored for a referenced
    /// `validator_id` — [`crate::active_key`] returned `None`.
    UnknownValidator {
        /// The referenced, unresolvable `validator_id`.
        validator_id: Digest,
    },
    /// A [`hn_crypto::SignatureEnvelope::verify`] call failed — wrong
    /// signature, algorithm mismatch, or an unsupported algorithm.
    SignatureVerificationFailed(IdentityError),
    /// A `QuorumCertificate.signer_commitment`'s byte length does not
    /// match `ceil(ordered_active_set.len() / 8)` for the active set
    /// supplied to verification (ADR-0012, "Decided: signer commitment
    /// bit-level encoding").
    SignerCommitmentLengthMismatch,
    /// A `QuorumCertificate.signer_commitment` has a set bit past the
    /// end of the supplied active set — a padding bit that must be zero
    /// (ADR-0012, "Decided: signer commitment bit-level encoding").
    SignerCommitmentPaddingBitSet,
    /// A decoded `StakePayloadV1.payload_version` does not match
    /// [`crate::stake_payload::STAKE_PAYLOAD_VERSION_1`], the only shape
    /// this implementation understands.
    UnsupportedStakePayloadVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `UnstakePayloadV1.payload_version` does not match
    /// [`crate::unstake_payload::UNSTAKE_PAYLOAD_VERSION_1`], the only
    /// shape this implementation understands.
    UnsupportedUnstakePayloadVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `ValidatorUpdatePayloadV1.payload_version` does not
    /// match [`crate::validator_update_payload::VALIDATOR_UPDATE_PAYLOAD_VERSION_1`],
    /// the only shape this implementation understands.
    UnsupportedValidatorUpdatePayloadVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `ValidatorUpdatePayloadV1.operation` byte is not a
    /// member of the closed `operation` registry (ADR-0006, "Decided:
    /// `stake`/`unstake`/`validator_update` payload shapes").
    InvalidValidatorOperation {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `ValidatorUpdatePayloadV1` has `new_consensus_key`
    /// present or absent in a way that does not match what `operation`
    /// requires — present only for `register`/`update_keys`, absent
    /// otherwise (ADR-0006, "Decided: `stake`/`unstake`/
    /// `validator_update` payload shapes").
    ValidatorUpdateKeyPresenceMismatch,
    /// Applying a `stake` would overflow the target `ValidatorRecordV1.
    /// bonded_stake`'s `u128` range.
    BondedStakeOverflow,
    /// A `stake`/`unstake` (ADR-0006, "Payload") would debit more than
    /// the target's current `bonded_stake` — the validation
    /// precondition "`bonded_stake` must be at least `amount`" failed.
    InsufficientBondedStake,
    /// A `validator_update { operation: register }` named a `sender`
    /// that already has a `ValidatorRecordV1` — `register` creates a
    /// new record, it does not update an existing one.
    ValidatorAlreadyRegistered,
    /// A `validator_update` operation is not valid from the target
    /// `ValidatorRecordV1`'s current `status` (for example `activate`
    /// on an already-`Active` record, or `exit` on anything but
    /// `Inactive`).
    InvalidValidatorStatusTransition {
        /// The record's current status byte.
        status: u8,
        /// The attempted operation byte.
        operation: u8,
    },
    /// A [`crate::StateReader`]/[`crate::StateWriter`] backend failed to
    /// read or write (ADR-0019, "initial storage backend": decided
    /// `redb`). Carries the backend's own `Display` output rather than a
    /// typed sub-error — per ADR-0019's "Backend Independence" rule, this
    /// crate's interfaces must not leak which specific backend failed,
    /// only that a storage operation did; an in-memory backend
    /// ([`crate::state_store`]'s own prior "deliberately infallible"
    /// framing) can never produce this, only a durable one.
    Storage(String),
    /// An `unstake` (ADR-0006, "Payload") was applied to a
    /// `ValidatorRecordV1` that already has a
    /// [`crate::ValidatorRecordV1::pending_unbonding`] withdrawal in
    /// progress — at most one pending withdrawal per validator is
    /// supported; a second `unstake` must wait for the first to mature
    /// (`crate::apply_unbonding_release`) before starting another.
    PendingUnbondingAlreadyExists,
    /// Computing an `unstake`'s unbonding maturity height (`current
    /// height + UNBONDING_PERIOD_BLOCKS`, ADR-0023's "Decided:
    /// Unbonding Period" converted to blocks via ADR-0009's "Decided:
    /// Target Block Time") would overflow `u64` — practically
    /// unreachable given realistic heights, but not silently wrapped.
    UnbondingMaturityHeightOverflow,
    /// A decoded `GovernancePayloadV1.payload_version` does not match
    /// [`crate::governance_payload::GOVERNANCE_PAYLOAD_VERSION_1`], the
    /// only shape this implementation understands.
    UnsupportedGovernancePayloadVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `GovernancePayloadV1.operation` byte is not a member
    /// of the closed `operation` registry (ADR-0025, "Decided:
    /// Transaction Payload Shape").
    InvalidGovernanceOperation {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `Vote` payload's `choice` byte is not a member of the
    /// closed `VoteChoice` registry (ADR-0025, "Decided: Transaction
    /// Payload Shape").
    InvalidVoteChoice {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `IdentityValueV1.identity_version` does not match
    /// [`crate::identity_value::IDENTITY_VERSION_1`], the only shape
    /// this implementation understands.
    UnsupportedIdentityVersion {
        /// The rejected version.
        value: u16,
    },
    /// [`crate::resolve_account_signing_key`] (ADR-0027) was given a
    /// present `bootstrap_key` for a `sender` that already has a stored
    /// `IdentityValueV1` — redundant once Identity State exists, not
    /// tolerated as harmless.
    UnexpectedBootstrapKey,
    /// [`crate::resolve_account_signing_key`] (ADR-0027) was given no
    /// `bootstrap_key` for a `sender` with no stored `IdentityValueV1`
    /// yet — nothing to verify the transaction's signature against.
    MissingBootstrapKey,
    /// [`crate::resolve_account_signing_key`] (ADR-0027) was given a
    /// `bootstrap_key` that does not derive `sender`'s own address
    /// (`hn_crypto::account_address_body`) — the anti-spoofing check:
    /// nobody may claim an address that is not the hash of the key they
    /// are presenting.
    BootstrapKeyAddressMismatch,
    /// [`crate::proposal_id`] was called with a `GovernancePayloadV1`
    /// whose operation is not `Propose` — `proposal_id` is only
    /// meaningful for a proposal's own creation content (ADR-0025,
    /// "Decided: `proposal_id` Derivation").
    ProposalIdRequiresProposeOperation,
    /// A `validator_update { operation: propose }` (ADR-0025) was
    /// submitted by a sender whose own `ValidatorRecordV1.status` is
    /// not `Active` — only active validators may propose ("Decided:
    /// Who May Propose").
    ProposerMustBeActive,
    /// A `Vote` (ADR-0025) was submitted by a sender with no
    /// `ValidatorRecordV1` at all, or one contributing zero weight to
    /// both governance chambers (`status != Active` and
    /// `bonded_stake == 0`) — nothing to cast a vote with, under the
    /// current (delegation-less) weight sources ("Decided: Chambers,
    /// Membership, And Weight").
    NoGovernanceVotingWeight,
    /// A `Vote` (ADR-0025) referenced a `proposal_id` this reader has
    /// no `ProposalRecordV1` for.
    UnknownProposal,
    /// A `Vote` (ADR-0025) was submitted by a sender who already has a
    /// `ProposalVoteRecordV1` for that `proposal_id` — "Decided:
    /// One Vote Per Sender Per Proposal" is not a silent overwrite.
    ProposalAlreadyVoted,
    /// A `Vote` (ADR-0025) was submitted against a `ProposalRecordV1`
    /// whose `status` is no longer `Voting`, or whose voting window
    /// (`voting_ends_at_height`) has already closed at the height the
    /// vote is being applied.
    VotingWindowClosed,
    /// `crate::apply_propose` was called with a `GovernancePayloadV1`
    /// whose operation is not `Propose`.
    ExpectedProposeOperation,
    /// `crate::apply_vote` was called with a `GovernancePayloadV1`
    /// whose operation is not `Vote`.
    ExpectedVoteOperation,
    /// Computing a proposal's `voting_ends_at_height`
    /// (`created_at_height + GOVERNANCE_VOTING_WINDOW`, ADR-0025/
    /// ADR-0023) would overflow `u64` — practically unreachable given
    /// realistic heights, but not silently wrapped.
    VotingWindowHeightOverflow,
    /// Accumulating a governance chamber tally or quorum computation
    /// would overflow `u128` — practically unreachable given realistic
    /// validator counts/stake amounts, but not silently wrapped.
    GovernanceTallyOverflow,
    /// A decoded `ProposalRecordV1.proposal_version` does not match
    /// [`crate::proposal_record::PROPOSAL_RECORD_VERSION_1`], the only
    /// shape this implementation understands.
    UnsupportedProposalRecordVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `ProposalRecordV1.status` byte is not a member of the
    /// closed `status` registry (ADR-0025, "Decided: Proposal Outcome
    /// States").
    InvalidProposalStatus {
        /// The rejected byte.
        value: u8,
    },
    /// A decoded `ProposalVoteRecordV1.vote_version` does not match
    /// [`crate::proposal_vote_record::PROPOSAL_VOTE_RECORD_VERSION_1`],
    /// the only shape this implementation understands.
    UnsupportedProposalVoteRecordVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `PermissionValueV1.permission_version` does not match
    /// [`crate::permission_value::PERMISSION_VERSION_1`], the only shape
    /// this implementation understands.
    UnsupportedPermissionVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `PermissionUpdatePayloadV1.payload_version` does not
    /// match
    /// [`crate::permission_update_payload::PERMISSION_UPDATE_PAYLOAD_VERSION_1`],
    /// the only shape this implementation understands.
    UnsupportedPermissionUpdatePayloadVersion {
        /// The rejected version.
        value: u16,
    },
    /// A `MultisigConfigV1` (ADR-0026) has `threshold == 0` or
    /// `threshold > authorized_keys.len()` — `1 <= threshold <=
    /// authorized_keys.len()` is a structural invariant, checked on
    /// both encode and decode.
    InvalidMultisigThreshold {
        /// The rejected `threshold` byte.
        threshold: u8,
        /// `authorized_keys.len()` at the time of the check.
        authorized_key_count: usize,
    },
    /// A `SignatureEnvelope` (`hn_crypto`) in a `signatures` list being
    /// checked against an active `MultisigConfigV1` (ADR-0026) has
    /// `key_reference: None` — every entry must carry a present
    /// `key_reference` once an account is in multisig mode; an entry
    /// shaped for single-key mode has no defined meaning there.
    MissingKeyReference,
    /// [`crate::TransactionEnvelope::verify`] (ADR-0027/ADR-0028) found
    /// a signer in single-key mode (no active `account_signing_multisig`)
    /// whose `signatures` list does not contain exactly one entry — the
    /// single-key model has exactly one active signature, unlike
    /// multisig mode's "at least `threshold`, extras tolerated" rule.
    ExpectedExactlyOneSignature {
        /// How many entries `signatures` actually had.
        count: usize,
    },
    /// [`crate::TransactionEnvelope::verify`] found a single-key-mode
    /// signature with a present `key_reference` — meaningless outside
    /// an active multisig configuration, the same "exactly one
    /// canonical encoding per semantic state" rejection
    /// [`StateError::MissingKeyReference`] enforces from the other
    /// direction.
    UnexpectedKeyReference,
    /// [`crate::verify_multisig_authorization`] (ADR-0026) found fewer
    /// distinct, in-bounds, successfully-verified `key_reference`s among
    /// the supplied `signatures` than `MultisigConfigV1.threshold`
    /// requires.
    InsufficientMultisigSignatures {
        /// `MultisigConfigV1.threshold`.
        required: u8,
        /// How many distinct signatures actually verified.
        valid: usize,
    },
    /// [`crate::apply_transaction`] (ADR-0030) found a
    /// `TransactionEnvelope.nonce` that does not exactly match
    /// `sender`'s current stored nonce — an inclusion precondition
    /// failure (ADR-0006, "Nonce": strictly increasing, gap-free), not
    /// a legitimate execution outcome.
    NonceMismatch {
        /// `sender`'s actual current nonce.
        expected: u64,
        /// The nonce the transaction carried.
        found: u64,
    },
    /// [`crate::apply_transaction`] (ADR-0030) found a transaction
    /// outside its own `validity_window` at `current_height` — an
    /// inclusion precondition failure, not a legitimate execution
    /// outcome.
    TransactionOutsideValidityWindow,
    /// [`crate::apply_transaction`] (ADR-0030) would overflow `u64`
    /// incrementing `sender`'s nonce — practically unreachable given
    /// realistic transaction counts, but not silently wrapped, the
    /// same class as [`StateError::UnbondingMaturityHeightOverflow`].
    NonceOverflow,
    /// A decoded `TransactionEnvelope.tx_version` does not match
    /// [`crate::transaction_envelope::TX_VERSION_1`], the only shape
    /// this implementation understands.
    UnsupportedTxVersion {
        /// The rejected version.
        value: u16,
    },
    /// A decoded `TransactionEnvelope.tx_type` byte is not a member of
    /// the closed `tx_type` registry (ADR-0006, "Decided: `tx_type`
    /// registry") — `0x00` is reserved and always invalid.
    InvalidTxType {
        /// The rejected byte.
        value: u8,
    },
    /// A raw `TransactionEnvelope` byte slice exceeds
    /// [`crate::transaction_envelope::MAX_TRANSACTION_SIZE`] (ADR-0006,
    /// "Decided: Transaction size limit") — checked before attempting
    /// to decode anything else.
    TransactionTooLarge {
        /// The rejected byte length.
        size: usize,
    },
    /// [`crate::decode_transaction_payload`] was called with a
    /// `tx_type` that has no decided payload schema yet
    /// (`contract_deploy`/`contract_call`/`system`) — a structurally
    /// valid `tx_type` registry value, not a malformed encoding.
    UndecidedTransactionPayload {
        /// The `tx_type` byte with no payload schema.
        tx_type: u8,
    },
}

impl From<HashError> for StateError {
    fn from(error: HashError) -> Self {
        Self::Hash(error)
    }
}

impl From<HncsError> for StateError {
    fn from(error: HncsError) -> Self {
        Self::Hash(HashError::Framing(error))
    }
}

impl core::fmt::Display for StateError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Hash(error) => write!(formatter, "state hashing error: {error}"),
            Self::DuplicateStateKey => formatter.write_str("duplicate state_key in write set"),
            Self::ReservedExtensionId => {
                formatter.write_str("extension_id 0x0000 is reserved for the registry leaf")
            }
            Self::Encoding(error) => write!(formatter, "value schema encoding error: {error}"),
            Self::InvalidAccountType { value } => {
                write!(formatter, "invalid account_type: 0x{value:02x}")
            }
            Self::UnsupportedEnvelopeVersion { value } => {
                write!(formatter, "unsupported envelope_version: {value}")
            }
            Self::UnsupportedNonceVersion { value } => {
                write!(formatter, "unsupported nonce_version: {value}")
            }
            Self::UnsupportedBalanceVersion { value } => {
                write!(formatter, "unsupported balance_version: {value}")
            }
            Self::UnsupportedAssetVersion { value } => {
                write!(formatter, "unsupported asset_version: {value}")
            }
            Self::UnsupportedLifecycleVersion { value } => {
                write!(formatter, "unsupported lifecycle_version: {value}")
            }
            Self::InvalidLifecycleState { value } => {
                write!(formatter, "invalid lifecycle state: 0x{value:02x}")
            }
            Self::UnsupportedTransferPayloadVersion { value } => {
                write!(formatter, "unsupported transfer payload_version: {value}")
            }
            Self::InsufficientBalance => formatter.write_str("insufficient balance for transfer"),
            Self::BalanceOverflow => {
                formatter.write_str("transfer credit would overflow recipient balance")
            }
            Self::UnsupportedReceiptVersion { value } => {
                write!(formatter, "unsupported receipt_version: {value}")
            }
            Self::InvalidReceiptStatus { value } => {
                write!(formatter, "invalid receipt status: 0x{value:02x}")
            }
            Self::UnsupportedVoteVersion { value } => {
                write!(formatter, "unsupported vote_version: {value}")
            }
            Self::UnsupportedConsensusProfile { value } => {
                write!(formatter, "unsupported consensus_profile: {value}")
            }
            Self::InvalidVoteType { value } => {
                write!(formatter, "invalid vote_type: 0x{value:02x}")
            }
            Self::InvalidVoteTargetType { value } => {
                write!(formatter, "invalid target_type: 0x{value:02x}")
            }
            Self::NonCanonicalNilTarget => {
                formatter.write_str("nil vote target_hash must be all-zero")
            }
            Self::UnsupportedQcVersion { value } => {
                write!(formatter, "unsupported qc_version: {value}")
            }
            Self::SignerCountMismatch => {
                formatter.write_str("aggregate_proof entry count does not match signer_commitment")
            }
            Self::SignedVotingPowerExceedsTotal => {
                formatter.write_str("signed_voting_power exceeds total_voting_power")
            }
            Self::UnsupportedRecordVersion { value } => {
                write!(formatter, "unsupported record_version: {value}")
            }
            Self::UnsupportedKeyAlgorithm { value } => {
                write!(formatter, "unsupported consensus_key algorithm_id: {value}")
            }
            Self::InvalidConsensusKey(error) => {
                write!(formatter, "invalid consensus_key: {error}")
            }
            Self::InvalidValidatorStatus { value } => {
                write!(formatter, "invalid validator status: 0x{value:02x}")
            }
            Self::InvalidSignatureEnvelope(error) => {
                write!(formatter, "invalid signature envelope: {error}")
            }
            Self::UnknownValidator { validator_id } => {
                write!(formatter, "unknown validator_id: {}", hex(validator_id))
            }
            Self::SignatureVerificationFailed(error) => {
                write!(formatter, "signature verification failed: {error}")
            }
            Self::SignerCommitmentLengthMismatch => {
                formatter.write_str("signer_commitment length does not match the active set size")
            }
            Self::SignerCommitmentPaddingBitSet => {
                formatter.write_str("signer_commitment has a padding bit set past the active set")
            }
            Self::UnsupportedStakePayloadVersion { value } => {
                write!(formatter, "unsupported stake payload_version: {value}")
            }
            Self::UnsupportedUnstakePayloadVersion { value } => {
                write!(formatter, "unsupported unstake payload_version: {value}")
            }
            Self::UnsupportedValidatorUpdatePayloadVersion { value } => {
                write!(
                    formatter,
                    "unsupported validator_update payload_version: {value}"
                )
            }
            Self::InvalidValidatorOperation { value } => {
                write!(
                    formatter,
                    "invalid validator_update operation: 0x{value:02x}"
                )
            }
            Self::ValidatorUpdateKeyPresenceMismatch => formatter
                .write_str("new_consensus_key presence does not match validator_update operation"),
            Self::BondedStakeOverflow => formatter.write_str("stake would overflow bonded_stake"),
            Self::InsufficientBondedStake => {
                formatter.write_str("insufficient bonded_stake for unstake")
            }
            Self::ValidatorAlreadyRegistered => {
                formatter.write_str("validator_update register: sender already has a record")
            }
            Self::InvalidValidatorStatusTransition { status, operation } => write!(
                formatter,
                "validator_update operation 0x{operation:02x} is invalid from status 0x{status:02x}"
            ),
            Self::Storage(message) => write!(formatter, "storage backend error: {message}"),
            Self::PendingUnbondingAlreadyExists => {
                formatter.write_str("a pending unbonding withdrawal already exists")
            }
            Self::UnbondingMaturityHeightOverflow => {
                formatter.write_str("unbonding maturity height would overflow u64")
            }
            Self::UnsupportedGovernancePayloadVersion { value } => {
                write!(formatter, "unsupported governance payload_version: {value}")
            }
            Self::InvalidGovernanceOperation { value } => {
                write!(formatter, "invalid governance operation: 0x{value:02x}")
            }
            Self::InvalidVoteChoice { value } => {
                write!(formatter, "invalid vote choice: 0x{value:02x}")
            }
            Self::UnsupportedIdentityVersion { value } => {
                write!(formatter, "unsupported identity_version: {value}")
            }
            Self::UnexpectedBootstrapKey => formatter
                .write_str("bootstrap_key present but sender already has an IdentityValueV1"),
            Self::MissingBootstrapKey => {
                formatter.write_str("bootstrap_key absent but sender has no IdentityValueV1 yet")
            }
            Self::BootstrapKeyAddressMismatch => {
                formatter.write_str("bootstrap_key does not derive sender's own address")
            }
            Self::ProposalIdRequiresProposeOperation => {
                formatter.write_str("proposal_id can only be computed for a Propose payload")
            }
            Self::ProposerMustBeActive => {
                formatter.write_str("only an Active validator may propose")
            }
            Self::NoGovernanceVotingWeight => {
                formatter.write_str("sender has no weight in either governance chamber")
            }
            Self::UnknownProposal => formatter.write_str("unknown proposal_id"),
            Self::ProposalAlreadyVoted => {
                formatter.write_str("sender already voted on this proposal")
            }
            Self::VotingWindowClosed => formatter.write_str("proposal's voting window is closed"),
            Self::UnsupportedProposalRecordVersion { value } => {
                write!(formatter, "unsupported proposal_version: {value}")
            }
            Self::InvalidProposalStatus { value } => {
                write!(formatter, "invalid proposal status: 0x{value:02x}")
            }
            Self::UnsupportedProposalVoteRecordVersion { value } => {
                write!(formatter, "unsupported vote_version: {value}")
            }
            Self::UnsupportedPermissionVersion { value } => {
                write!(formatter, "unsupported permission_version: {value}")
            }
            Self::UnsupportedPermissionUpdatePayloadVersion { value } => {
                write!(
                    formatter,
                    "unsupported permission_update payload_version: {value}"
                )
            }
            Self::InvalidMultisigThreshold {
                threshold,
                authorized_key_count,
            } => write!(
                formatter,
                "invalid multisig threshold {threshold} for {authorized_key_count} authorized keys"
            ),
            Self::MissingKeyReference => {
                formatter.write_str("signature envelope has no key_reference in multisig mode")
            }
            Self::ExpectedExactlyOneSignature { count } => write!(
                formatter,
                "expected exactly one signature in single-key mode, got {count}"
            ),
            Self::UnexpectedKeyReference => {
                formatter.write_str("key_reference present on a single-key-mode signature")
            }
            Self::InsufficientMultisigSignatures { required, valid } => write!(
                formatter,
                "insufficient multisig signatures: {valid} valid, {required} required"
            ),
            Self::ExpectedProposeOperation => {
                formatter.write_str("apply_propose requires a Propose payload")
            }
            Self::ExpectedVoteOperation => {
                formatter.write_str("apply_vote requires a Vote payload")
            }
            Self::VotingWindowHeightOverflow => {
                formatter.write_str("voting_ends_at_height would overflow u64")
            }
            Self::GovernanceTallyOverflow => {
                formatter.write_str("governance chamber tally would overflow u128")
            }
            Self::NonceMismatch { expected, found } => write!(
                formatter,
                "nonce mismatch: expected {expected}, found {found}"
            ),
            Self::TransactionOutsideValidityWindow => {
                formatter.write_str("transaction is outside its own validity_window")
            }
            Self::NonceOverflow => {
                formatter.write_str("incrementing sender's nonce would overflow u64")
            }
            Self::UnsupportedTxVersion { value } => {
                write!(formatter, "unsupported tx_version: {value}")
            }
            Self::InvalidTxType { value } => {
                write!(formatter, "invalid tx_type: 0x{value:02x}")
            }
            Self::TransactionTooLarge { size } => write!(
                formatter,
                "transaction of {size} bytes exceeds MAX_TRANSACTION_SIZE"
            ),
            Self::UndecidedTransactionPayload { tx_type } => write!(
                formatter,
                "tx_type 0x{tx_type:02x} has no decided payload schema yet"
            ),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl std::error::Error for StateError {}
