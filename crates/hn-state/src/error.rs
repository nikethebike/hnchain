use hn_crypto::HashError;
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
        }
    }
}

impl std::error::Error for StateError {}
