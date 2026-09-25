use hn_core::ProtocolVersion;
use hn_crypto::{Digest, hash_profile_0x0001};
use hn_hncs::{Decoder, write_bytes, write_fixed_bytes, write_u8, write_u16};

use crate::error::{NetworkError, NetworkResult};
use crate::registry::{Channel, MessageType};

/// `envelope_version` for the current [`P2PMessageEnvelopeV1`] shape
/// (ADR-0022's Structure Version convention).
pub const ENVELOPE_VERSION_1: u16 = 1;

/// Maximum length, in bytes, of `P2PMessageEnvelopeV1.payload`. An
/// implementation resource bound picked with headroom (the same class
/// of decision as `MAX_TRANSACTION_SIZE`/`MAX_BLOCK_SIZE`), matching
/// `hn_state`'s own `MAX_BLOCK_SIZE` exactly since the largest payload
/// this profile carries (`BlockResponseV1`) is roughly "a block's worth
/// of data."
pub const MAX_PAYLOAD_LEN: usize = 8_388_608;

/// A versioned P2P message envelope (ADR-0018, "Decided"; ADR-0036,
/// "Decided: Message Envelope") — every field ADR-0018's own conceptual
/// sketch names, except `capabilities`/`compression_profile`/
/// `encryption_profile`/`authentication`, deliberately not carried at
/// this layer for V1 (see ADR-0036's own "Decided: Message Envelope"
/// for why each is either message-specific or has nothing decided yet
/// to put in it).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P2PMessageEnvelopeV1 {
    /// Structure version for this envelope shape.
    pub envelope_version: u16,
    /// The sender's own protocol version.
    pub protocol_version: ProtocolVersion,
    /// HNChain protocol lineage (ADR-0006, "Chain And Network
    /// Binding") — raw `u8`, matching `TransactionEnvelope`/
    /// `VoteSigningPayloadV1`'s own established field convention
    /// rather than the separate `hn_core::ChainId` newtype.
    pub chain_id: u8,
    /// Network environment (ADR-0003).
    pub network_id: u16,
    /// Which logical channel this message belongs to.
    pub channel: Channel,
    /// Which message family `payload` decodes as.
    pub message_type: MessageType,
    /// Content-addressed message identifier — see
    /// [`compute_message_id`].
    pub message_id: Digest,
    /// `message_type`-discriminated payload bytes, already canonical.
    pub payload: Vec<u8>,
}

impl P2PMessageEnvelopeV1 {
    /// Builds an envelope, computing `message_id` from `channel`,
    /// `message_type`, and `payload` (see [`compute_message_id`]) —
    /// the only way to construct one, so `message_id` can never drift
    /// from the content it identifies.
    pub fn new(
        protocol_version: ProtocolVersion,
        chain_id: u8,
        network_id: u16,
        channel: Channel,
        message_type: MessageType,
        payload: Vec<u8>,
    ) -> NetworkResult<Self> {
        let message_id = compute_message_id(channel, message_type, &payload)?;
        Ok(Self {
            envelope_version: ENVELOPE_VERSION_1,
            protocol_version,
            chain_id,
            network_id,
            channel,
            message_type,
            message_id,
            payload,
        })
    }

    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, self.envelope_version);
        write_u16(&mut out, self.protocol_version.major());
        write_u16(&mut out, self.protocol_version.minor());
        write_u16(&mut out, self.protocol_version.patch());
        write_u8(&mut out, self.chain_id);
        write_u16(&mut out, self.network_id);
        write_u8(&mut out, self.channel.as_u8());
        write_u16(&mut out, self.message_type.as_u16());
        write_fixed_bytes(&mut out, &self.message_id);
        write_bytes(&mut out, &self.payload, MAX_PAYLOAD_LEN)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`P2PMessageEnvelopeV1::encode`] — ADR-0018's own "Cheap
    /// Rejection" stages this function alone covers: frame/payload
    /// length limit (via `write_bytes`'s own bound,
    /// [`NetworkError::Encoding`] on violation), version check
    /// ([`NetworkError::UnsupportedEnvelopeVersion`]), channel/type
    /// check ([`NetworkError::UnsupportedChannel`]/
    /// [`NetworkError::UnsupportedMessageType`]), and payload hash
    /// check ([`NetworkError::MessageIdMismatch`]). Chain/network
    /// binding and authentication prechecks are the caller's job —
    /// this function has no context for "this node's own chain_id" or
    /// any connection/session state.
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let envelope_version = decoder.read_u16()?;
        if envelope_version != ENVELOPE_VERSION_1 {
            return Err(NetworkError::UnsupportedEnvelopeVersion {
                value: envelope_version,
            });
        }
        let major = decoder.read_u16()?;
        let minor = decoder.read_u16()?;
        let patch = decoder.read_u16()?;
        let protocol_version = ProtocolVersion::new(major, minor, patch);
        let chain_id = decoder.read_u8()?;
        let network_id = decoder.read_u16()?;
        let channel = Channel::from_u8(decoder.read_u8()?)?;
        let message_type = MessageType::from_u16(decoder.read_u16()?)?;
        let message_id = decoder.read_fixed_bytes::<32>()?;
        let payload = decoder.read_bytes(MAX_PAYLOAD_LEN)?.to_vec();

        decoder.finish()?;

        let expected_message_id = compute_message_id(channel, message_type, &payload)?;
        if message_id != expected_message_id {
            return Err(NetworkError::MessageIdMismatch);
        }

        Ok(Self {
            envelope_version,
            protocol_version,
            chain_id,
            network_id,
            channel,
            message_type,
            message_id,
            payload,
        })
    }
}

/// Computes a content-addressed message identifier (ADR-0036, "Decided:
/// Message Envelope"):
///
/// `message_id = HASH_PROFILE_0x0001("hnchain.p2p.message.v1",
/// HNCS(channel || message_type || payload))`
///
/// Two peers relaying the exact same announcement produce identical
/// `message_id`s — useful for gossip deduplication ("have I already
/// seen this message") — since it depends only on content, not on
/// which peer sent it or when.
pub fn compute_message_id(
    channel: Channel,
    message_type: MessageType,
    payload: &[u8],
) -> NetworkResult<Digest> {
    let mut preimage = Vec::new();
    write_u8(&mut preimage, channel.as_u8());
    write_u16(&mut preimage, message_type.as_u16());
    write_bytes(&mut preimage, payload, MAX_PAYLOAD_LEN)?;

    Ok(hash_profile_0x0001("hnchain.p2p.message.v1", &preimage)?)
}

#[cfg(test)]
mod tests {
    use hn_core::ProtocolVersion;

    use super::{ENVELOPE_VERSION_1, P2PMessageEnvelopeV1};
    use crate::error::{NetworkError, NetworkResult};
    use crate::registry::{Channel, MessageType};

    fn sample() -> NetworkResult<P2PMessageEnvelopeV1> {
        P2PMessageEnvelopeV1::new(
            ProtocolVersion::new(0, 1, 0),
            1,
            1,
            Channel::Transactions,
            MessageType::TransactionAnnounce,
            b"payload".to_vec(),
        )
    }

    #[test]
    fn round_trips_through_decode() -> NetworkResult<()> {
        let envelope = sample()?;
        let decoded = P2PMessageEnvelopeV1::decode(&envelope.encode()?)?;
        assert_eq!(decoded, envelope);
        Ok(())
    }

    #[test]
    fn message_id_matches_independent_oracle() -> NetworkResult<()> {
        let id = super::compute_message_id(
            Channel::Transactions,
            MessageType::TransactionAnnounce,
            b"payload",
        )?;
        assert_eq!(
            hex(&id),
            "7fe6e19e989dd0af0adf6ce5c141c75786b619bc77aab552b0381ff3145697eb"
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn rejects_unsupported_envelope_version() -> NetworkResult<()> {
        let mut encoded = sample()?.encode()?;
        encoded[0] = 0x02; // envelope_version low byte, little-endian
        assert_eq!(
            P2PMessageEnvelopeV1::decode(&encoded),
            Err(NetworkError::UnsupportedEnvelopeVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_a_tampered_payload_via_message_id_mismatch() -> NetworkResult<()> {
        let envelope = sample()?;
        let mut encoded = envelope.encode()?;
        // Flip a byte inside the payload, past the fixed-size header +
        // message_id -- the length-prefixed payload tail.
        let last = encoded.len() - 1;
        encoded[last] ^= 0xFF;
        assert_eq!(
            P2PMessageEnvelopeV1::decode(&encoded),
            Err(NetworkError::MessageIdMismatch)
        );
        Ok(())
    }

    #[test]
    fn distinct_content_produces_distinct_message_ids() -> NetworkResult<()> {
        let a = sample()?;
        let mut b = sample()?;
        b.payload = b"different".to_vec();
        let b = P2PMessageEnvelopeV1::new(
            b.protocol_version,
            b.chain_id,
            b.network_id,
            b.channel,
            b.message_type,
            b.payload,
        )?;
        assert_ne!(a.message_id, b.message_id);
        Ok(())
    }

    #[test]
    fn envelope_version_is_1_by_default() -> NetworkResult<()> {
        assert_eq!(sample()?.envelope_version, ENVELOPE_VERSION_1);
        Ok(())
    }
}
