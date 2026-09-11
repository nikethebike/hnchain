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
    /// A decoded `ValidatorRecordV1.consensus_key`'s `algorithm_id` is
    /// not Ed25519 (`hn_crypto::ED25519_ALGORITHM_ID`), the only active
    /// `validator_consensus` signing suite at genesis (ADR-0002,
    /// "Accepted Initial Direction").
    UnsupportedKeyAlgorithm {
        /// The rejected algorithm identifier.
        value: u16,
    },
    /// A decoded `ValidatorRecordV1.consensus_key`'s public key bytes
    /// were rejected by [`hn_crypto::KeyDescriptor`] — wrong length for
    /// the declared algorithm, or not a canonical Ed25519 point.
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
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl std::error::Error for StateError {}
