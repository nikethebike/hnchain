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
        }
    }
}

impl std::error::Error for StateError {}
