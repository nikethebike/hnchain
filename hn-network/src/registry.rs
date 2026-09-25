use crate::error::NetworkError;

/// The `channel` registry (ADR-0018, "Channel Separation"; ADR-0036,
/// "Decided: Channel And Message Type Registries"), closed for this
/// profile. Every one of ADR-0018's own ten named conceptual channels
/// gets an ID here, including the five this pass does not implement —
/// reserving the registry slot and naming the real blocker, the same
/// pattern [`hn_state::TxType`](../hn_state/enum.TxType.html) already
/// used for `contract_deploy`/`contract_call`/`system`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u8)]
pub enum Channel {
    /// `Hello` handshake/capability exchange.
    Handshake = 0x01,
    /// `PeerAnnounce`/`PeerRequest` gossip.
    PeerDiscovery = 0x02,
    /// `TransactionAnnounce`/`TransactionRequest`/`TransactionResponse`.
    Transactions = 0x03,
    /// `BlockAnnounce`/`BlockRequest`/`BlockResponse`.
    Blocks = 0x04,
    /// `ConsensusProposal`/`ConsensusVote`/`QuorumCertificateMessage`.
    Consensus = 0x05,
    /// Reserved: blocked on ADR-0015's own evidence message design.
    Evidence = 0x06,
    /// Reserved: blocked on ADR-0016's own sync packet design.
    Sync = 0x07,
    /// Reserved: blocked on ADR-0016's own snapshot packet design.
    Snapshot = 0x08,
    /// Reserved: blocked on ADR-0017's own light-client proof design.
    LightClient = 0x09,
    /// `Ping`/`Pong`/`Disconnect` — reserved: no payload schema or
    /// handling logic in this pass.
    Control = 0x0A,
}

impl Channel {
    /// Returns the registry value for this channel.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a registry value, rejecting anything unrecognized
    /// ([`NetworkError::UnsupportedChannel`]).
    pub fn from_u8(value: u8) -> Result<Self, NetworkError> {
        match value {
            0x01 => Ok(Self::Handshake),
            0x02 => Ok(Self::PeerDiscovery),
            0x03 => Ok(Self::Transactions),
            0x04 => Ok(Self::Blocks),
            0x05 => Ok(Self::Consensus),
            0x06 => Ok(Self::Evidence),
            0x07 => Ok(Self::Sync),
            0x08 => Ok(Self::Snapshot),
            0x09 => Ok(Self::LightClient),
            0x0A => Ok(Self::Control),
            _ => Err(NetworkError::UnsupportedChannel { value }),
        }
    }
}

/// The `message_type` registry (ADR-0018, "Message Types"; ADR-0036,
/// "Decided: Channel And Message Type Registries"), closed for this
/// profile, global across channels (not re-scoped per channel) — the
/// same "one flat registry" shape `TxType` already uses. Merges the
/// RFC's own separate `hello`/`capabilities` messages into one
/// [`MessageType::Hello`], and its separate `block_header_request`/
/// `block_body_request` into one [`MessageType::BlockRequest`] (no
/// concrete `BlockHeader` type exists anywhere in this codebase yet —
/// see ADR-0036's own "Decided: Block/Transaction/Vote Propagation").
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u16)]
pub enum MessageType {
    /// Handshake: identity, capabilities, and chain/network binding in
    /// one message each direction.
    Hello = 0x0001,
    /// Gossip: "here are peers I know about."
    PeerAnnounce = 0x0002,
    /// Gossip: "tell me what peers you know about."
    PeerRequest = 0x0003,
    /// "I have this transaction" (`tx_id` only).
    TransactionAnnounce = 0x0004,
    /// "Send me this transaction."
    TransactionRequest = 0x0005,
    /// The requested transaction's full canonical bytes.
    TransactionResponse = 0x0006,
    /// "I have this block" (`block_hash` only).
    BlockAnnounce = 0x0007,
    /// "Send me this block."
    BlockRequest = 0x0008,
    /// The requested block's transaction list.
    BlockResponse = 0x0009,
    /// A consensus round proposal.
    ConsensusProposal = 0x000A,
    /// An individual consensus vote.
    ConsensusVote = 0x000B,
    /// A quorum certificate.
    QuorumCertificateMessage = 0x000C,
    /// Reserved: no payload schema or handling logic this pass.
    Ping = 0x000D,
    /// Reserved: no payload schema or handling logic this pass.
    Pong = 0x000E,
    /// Reserved: no payload schema or handling logic this pass.
    Disconnect = 0x000F,
    /// Reserved: blocked on ADR-0015's own evidence message design.
    EvidenceAnnounce = 0x0010,
    /// Reserved: blocked on ADR-0015's own evidence message design.
    EvidenceRequest = 0x0011,
    /// Reserved: blocked on ADR-0016's own checkpoint packet design.
    CheckpointRequest = 0x0012,
    /// Reserved: blocked on ADR-0016's own checkpoint packet design.
    CheckpointResponse = 0x0013,
    /// Reserved: blocked on ADR-0016's own snapshot packet design.
    SnapshotManifest = 0x0014,
    /// Reserved: blocked on ADR-0016's own snapshot packet design.
    SnapshotChunkRequest = 0x0015,
    /// Reserved: blocked on ADR-0016's own snapshot packet design.
    SnapshotChunkResponse = 0x0016,
    /// Reserved: blocked on ADR-0017's own light-client proof design.
    LightClientProofRequest = 0x0017,
    /// Reserved: blocked on ADR-0017's own light-client proof design.
    LightClientProofResponse = 0x0018,
}

impl MessageType {
    /// Returns the registry value for this message type.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    /// Decodes a registry value, rejecting anything unrecognized
    /// ([`NetworkError::UnsupportedMessageType`]).
    pub fn from_u16(value: u16) -> Result<Self, NetworkError> {
        match value {
            0x0001 => Ok(Self::Hello),
            0x0002 => Ok(Self::PeerAnnounce),
            0x0003 => Ok(Self::PeerRequest),
            0x0004 => Ok(Self::TransactionAnnounce),
            0x0005 => Ok(Self::TransactionRequest),
            0x0006 => Ok(Self::TransactionResponse),
            0x0007 => Ok(Self::BlockAnnounce),
            0x0008 => Ok(Self::BlockRequest),
            0x0009 => Ok(Self::BlockResponse),
            0x000A => Ok(Self::ConsensusProposal),
            0x000B => Ok(Self::ConsensusVote),
            0x000C => Ok(Self::QuorumCertificateMessage),
            0x000D => Ok(Self::Ping),
            0x000E => Ok(Self::Pong),
            0x000F => Ok(Self::Disconnect),
            0x0010 => Ok(Self::EvidenceAnnounce),
            0x0011 => Ok(Self::EvidenceRequest),
            0x0012 => Ok(Self::CheckpointRequest),
            0x0013 => Ok(Self::CheckpointResponse),
            0x0014 => Ok(Self::SnapshotManifest),
            0x0015 => Ok(Self::SnapshotChunkRequest),
            0x0016 => Ok(Self::SnapshotChunkResponse),
            0x0017 => Ok(Self::LightClientProofRequest),
            0x0018 => Ok(Self::LightClientProofResponse),
            _ => Err(NetworkError::UnsupportedMessageType { value }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Channel, MessageType};
    use crate::error::NetworkError;

    #[test]
    fn every_channel_round_trips_through_its_registry_value() {
        for channel in [
            Channel::Handshake,
            Channel::PeerDiscovery,
            Channel::Transactions,
            Channel::Blocks,
            Channel::Consensus,
            Channel::Evidence,
            Channel::Sync,
            Channel::Snapshot,
            Channel::LightClient,
            Channel::Control,
        ] {
            assert_eq!(Channel::from_u8(channel.as_u8()), Ok(channel));
        }
    }

    #[test]
    fn rejects_an_unrecognized_channel() {
        assert_eq!(
            Channel::from_u8(0x00),
            Err(NetworkError::UnsupportedChannel { value: 0x00 })
        );
        assert_eq!(
            Channel::from_u8(0xFF),
            Err(NetworkError::UnsupportedChannel { value: 0xFF })
        );
    }

    #[test]
    fn every_message_type_round_trips_through_its_registry_value() {
        for message_type in [
            MessageType::Hello,
            MessageType::PeerAnnounce,
            MessageType::PeerRequest,
            MessageType::TransactionAnnounce,
            MessageType::TransactionRequest,
            MessageType::TransactionResponse,
            MessageType::BlockAnnounce,
            MessageType::BlockRequest,
            MessageType::BlockResponse,
            MessageType::ConsensusProposal,
            MessageType::ConsensusVote,
            MessageType::QuorumCertificateMessage,
            MessageType::Ping,
            MessageType::Pong,
            MessageType::Disconnect,
            MessageType::EvidenceAnnounce,
            MessageType::EvidenceRequest,
            MessageType::CheckpointRequest,
            MessageType::CheckpointResponse,
            MessageType::SnapshotManifest,
            MessageType::SnapshotChunkRequest,
            MessageType::SnapshotChunkResponse,
            MessageType::LightClientProofRequest,
            MessageType::LightClientProofResponse,
        ] {
            assert_eq!(
                MessageType::from_u16(message_type.as_u16()),
                Ok(message_type)
            );
        }
    }

    #[test]
    fn rejects_an_unrecognized_message_type() {
        assert_eq!(
            MessageType::from_u16(0x0000),
            Err(NetworkError::UnsupportedMessageType { value: 0x0000 })
        );
        assert_eq!(
            MessageType::from_u16(0xFFFF),
            Err(NetworkError::UnsupportedMessageType { value: 0xFFFF })
        );
    }
}
