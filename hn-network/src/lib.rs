#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! P2P networking boundaries for HNChain.
//!
//! This crate owns transport-facing abstractions and must not define consensus
//! validity.
//!
//! ADR-0036, "Basic P2P Networking," is this crate's first real
//! content, deliberately scoped to node discovery, peer handshake, and
//! message serialization for block/transaction/vote propagation — and
//! deliberately **pure protocol logic only**: no real transport, no
//! real sockets, no async runtime. Every type here is testable in
//! isolation, mirroring `hn_consensus::ConsensusState`'s own "pure
//! state machine first, real wiring later" split.
//!
//! [`envelope::P2PMessageEnvelopeV1`] is the versioned message wrapper
//! (ADR-0018) every other payload travels inside, carrying
//! `chain_id`/`network_id` binding, a [`registry::Channel`]/
//! [`registry::MessageType`] pair (both closed registries, reserving
//! IDs for the message families this pass does not implement rather
//! than leaving them undefined), and a content-addressed `message_id`
//! ([`envelope::compute_message_id`]) for gossip deduplication.
//! Deliberately does **not** carry generic `capabilities`/
//! `compression_profile`/`encryption_profile`/`authentication` fields
//! ADR-0018's own conceptual sketch lists on every message — see
//! ADR-0036's own "Decided: Message Envelope" for why each is either
//! message-specific or has nothing decided yet to put in it.
//!
//! [`identity::peer_id`] derives a P2P peer identifier from a
//! [`hn_crypto::KeyDescriptor`] holding the new
//! `hn_crypto::KeyRole::NodeIdentity` role (ADR-0036, "Decided: Node
//! Identity") — deliberately **not** network-bound, unlike
//! `hn_crypto::account_address_body`: peer identity is not a
//! consensus-visible object needing cross-network replay protection.
//!
//! [`handshake::Hello`]/[`handshake::HandshakeState`] implement the
//! peer handshake (ADR-0036, "Decided: Handshake") — one signed message
//! each direction (merging the RFC's own separate `hello`/
//! `capabilities` messages), checked via a pure state machine mirroring
//! `ConsensusState`'s own shape: `Idle -> SentHello`, `{Idle,
//! SentHello} -> {Established, Rejected}`, a deliberately closed, total
//! `apply`. A legitimate rejection (chain mismatch, missing capability,
//! bad signature) is `Ok(HandshakeAction::Rejected(reason))`, not an
//! `Err` — the same "legitimate outcome vs. inclusion-precondition
//! failure" distinction `hn_state`'s own `apply_*` functions already
//! draw; only an out-of-step event is a hard `Err`.
//!
//! [`discovery::PeerTable`] implements gossip-based discovery
//! (ADR-0036, "Decided: Discovery") — a bounded, pure in-memory table
//! seeded from a static bootstrap list, grown via
//! [`discovery::PeerAnnounceV1`]/[`discovery::PeerRequestV1`], no
//! scoring or reachability verification.
//!
//! [`propagation`] adds the block/transaction/vote propagation payload
//! shapes — [`propagation::TransactionResponseV1`]/
//! [`propagation::BlockResponseV1`]/
//! [`propagation::ConsensusProposalMessageV1`] deliberately embed
//! `hn_state::TransactionEnvelope`/`hn_state::QuorumCertificate`
//! directly rather than redefining them, and `ConsensusProposalMessageV1`'s
//! own three fields are deliberately identical in shape to
//! `hn_consensus::ConsensusEngine::handle_proposal`'s own parameters,
//! so a future wiring pass can hand one straight to it unchanged. An
//! individual `hn_state::ConsensusVote`/`QuorumCertificate` travels as
//! its own canonical bytes directly under
//! [`registry::MessageType::ConsensusVote`]/
//! [`registry::MessageType::QuorumCertificateMessage`] — no wrapper
//! struct needed.
//!
//! Explicitly out of scope, named rather than guessed at: any real
//! transport, connection/session model (and therefore `Hello` replay
//! protection), peer scoring/rate limiting, compression/encryption,
//! and the evidence/sync/checkpoint/snapshot/light-client message
//! families (each blocked on its own owning ADR having a concrete
//! object to carry) — see ADR-0036's own "Explicitly Not Resolved."

mod discovery;
mod envelope;
mod error;
mod handshake;
mod identity;
mod propagation;
mod registry;

pub use discovery::{
    MAX_ANNOUNCED_PEERS, MAX_KNOWN_PEERS, MAX_NETWORK_ADDRESS_LEN, PeerAddressV1, PeerAnnounceV1,
    PeerRequestV1, PeerTable,
};
pub use envelope::{ENVELOPE_VERSION_1, MAX_PAYLOAD_LEN, P2PMessageEnvelopeV1, compute_message_id};
pub use error::{NetworkError, NetworkResult};
pub use handshake::{
    HELLO_VERSION_1, HandshakeAction, HandshakeEvent, HandshakeParams, HandshakeState,
    HandshakeStep, Hello, HelloSigningPayloadV1, MAX_CAPABILITY_COUNT,
};
pub use identity::peer_id;
pub use propagation::{
    BlockAnnounceV1, BlockRequestV1, BlockResponseV1, ConsensusProposalMessageV1, MAX_QC_BLOB_LEN,
    MAX_TRANSACTION_BLOB_LEN, MAX_TRANSACTIONS_PER_MESSAGE, TransactionAnnounceV1,
    TransactionRequestV1, TransactionResponseV1,
};
pub use registry::{Channel, MessageType};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
