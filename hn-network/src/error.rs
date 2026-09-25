use std::fmt;

use hn_crypto::{HashError, IdentityError};
use hn_hncs::HncsError;
use hn_state::StateError;

use crate::registry::{Channel, MessageType};

/// Errors this crate's encoding, identity, and pure protocol-logic
/// operations can produce (ADR-0036, "Basic P2P Networking").
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkError {
    /// A canonical HNCS encode/decode operation failed.
    Encoding(HncsError),
    /// A hash-profile operation failed (domain tag, canonical payload
    /// bound).
    Hash(HashError),
    /// A signature/key-descriptor operation failed.
    Identity(IdentityError),
    /// Decoding an embedded `hn_state` protocol object
    /// (`TransactionEnvelope`/`ConsensusVote`/`QuorumCertificate`)
    /// failed.
    StateApplication(StateError),
    /// `envelope_version` is not [`crate::envelope::ENVELOPE_VERSION_1`],
    /// the only shape this implementation understands.
    UnsupportedEnvelopeVersion {
        /// The rejected version.
        value: u16,
    },
    /// A `channel` byte is not a recognized [`Channel`] registry value.
    UnsupportedChannel {
        /// The rejected value.
        value: u8,
    },
    /// A `message_type` value is not a recognized [`MessageType`]
    /// registry value.
    UnsupportedMessageType {
        /// The rejected value.
        value: u16,
    },
    /// `envelope.payload`'s own hash does not match `envelope.message_id`
    /// (ADR-0018, "Cheap Rejection": "payload hash check").
    MessageIdMismatch,
    /// `HandshakeState::apply` received an event with no defined
    /// transition from the current state — a deliberately closed, total
    /// function (ADR-0036, "Decided: Handshake"), mirroring
    /// `hn_consensus::ConsensusState::apply`'s own "reject, never
    /// silently ignore" discipline.
    UnexpectedHandshakeEvent,
    /// A peer's `Hello` declares a different `chain_id` than this
    /// node's own.
    ChainMismatch {
        /// This node's own `chain_id`.
        expected: u8,
        /// The peer's declared `chain_id`.
        found: u8,
    },
    /// A peer's `Hello` declares a different `network_id` than this
    /// node's own.
    NetworkMismatch {
        /// This node's own `network_id`.
        expected: u16,
        /// The peer's declared `network_id`.
        found: u16,
    },
    /// A peer's `Hello` does not advertise every channel this node
    /// requires (ADR-0018, "Capability Negotiation": "unknown required
    /// capabilities cause deterministic disconnect").
    MissingRequiredChannel {
        /// The unmet channel requirement.
        channel: Channel,
    },
    /// A peer's `Hello` does not advertise every message type this node
    /// requires. See [`NetworkError::MissingRequiredChannel`].
    MissingRequiredMessageType {
        /// The unmet message-type requirement.
        message_type: MessageType,
    },
    /// A `PeerRequestV1`'s own bytes were non-empty, despite that
    /// message type having no fields.
    UnexpectedPeerRequestPayload,
    /// A TCP frame's length prefix exceeds
    /// [`crate::transport::MAX_FRAME_LEN`] — rejected before allocating
    /// a buffer for it (ADR-0018, "Cheap Rejection": "frame limit," the
    /// very first stage, ahead of envelope decode).
    FrameTooLarge {
        /// The rejected length prefix.
        length: usize,
    },
    /// A real transport I/O operation failed (connection reset, broken
    /// pipe, and so on). Carries `std::io::Error`'s own message as an
    /// opaque `String` — mirroring `hn_state::StateError::Storage`'s own
    /// established boundary for a real backend error type that itself
    /// implements neither `Clone` nor `Eq`/`PartialEq`.
    Io(String),
    /// A [`crate::transport::PeerLink`]'s outbound channel has no
    /// receiver left — the connection's writer thread has already
    /// exited.
    PeerConnectionClosed,
}

impl fmt::Display for NetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encoding(error) => write!(formatter, "encoding error: {error}"),
            Self::Hash(error) => write!(formatter, "hash error: {error}"),
            Self::Identity(error) => write!(formatter, "identity error: {error}"),
            Self::StateApplication(error) => write!(formatter, "state application error: {error}"),
            Self::UnsupportedEnvelopeVersion { value } => {
                write!(formatter, "unsupported envelope_version {value}")
            }
            Self::UnsupportedChannel { value } => write!(formatter, "unsupported channel {value}"),
            Self::UnsupportedMessageType { value } => {
                write!(formatter, "unsupported message_type {value}")
            }
            Self::MessageIdMismatch => write!(formatter, "message_id does not match payload hash"),
            Self::UnexpectedHandshakeEvent => {
                write!(
                    formatter,
                    "unexpected event for the current handshake state"
                )
            }
            Self::ChainMismatch { expected, found } => {
                write!(
                    formatter,
                    "chain_id mismatch: expected {expected}, found {found}"
                )
            }
            Self::NetworkMismatch { expected, found } => {
                write!(
                    formatter,
                    "network_id mismatch: expected {expected}, found {found}"
                )
            }
            Self::MissingRequiredChannel { channel } => {
                write!(
                    formatter,
                    "peer does not support required channel {channel:?}"
                )
            }
            Self::MissingRequiredMessageType { message_type } => {
                write!(
                    formatter,
                    "peer does not support required message type {message_type:?}"
                )
            }
            Self::UnexpectedPeerRequestPayload => {
                write!(formatter, "PeerRequestV1 payload must be empty")
            }
            Self::FrameTooLarge { length } => {
                write!(
                    formatter,
                    "TCP frame length {length} exceeds the frame limit"
                )
            }
            Self::Io(message) => write!(formatter, "network I/O error: {message}"),
            Self::PeerConnectionClosed => {
                write!(formatter, "peer connection's writer thread has exited")
            }
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<HncsError> for NetworkError {
    fn from(error: HncsError) -> Self {
        Self::Encoding(error)
    }
}

impl From<HashError> for NetworkError {
    fn from(error: HashError) -> Self {
        Self::Hash(error)
    }
}

impl From<IdentityError> for NetworkError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

impl From<StateError> for NetworkError {
    fn from(error: StateError) -> Self {
        Self::StateApplication(error)
    }
}

/// Shorthand for `Result<T, NetworkError>`.
pub type NetworkResult<T> = Result<T, NetworkError>;
