use hn_core::ProtocolVersion;
use hn_crypto::{
    Digest, ED25519_ALGORITHM_ID, ED25519_PUBLIC_KEY_LEN, Ed25519KeyPair, KeyDescriptor, KeyRole,
    PUBLIC_KEY_MAX_LEN, SignatureEnvelope, hash_profile_0x0001,
};
use hn_hncs::{Decoder, write_bytes, write_u8, write_u16};

use crate::error::{NetworkError, NetworkResult};
use crate::identity::peer_id;
use crate::registry::{Channel, MessageType};

/// `hello_version` for the current [`HelloSigningPayloadV1`]/[`Hello`]
/// shape.
pub const HELLO_VERSION_1: u16 = 1;

/// Maximum number of entries in `supported_channels`/
/// `supported_message_types`. An implementation resource bound with
/// headroom over the closed registries' own current size (10
/// channels, 24 message types) — same class of decision as
/// `MAX_ACCESS_LIST_ENTRIES`.
pub const MAX_CAPABILITY_COUNT: usize = 64;

/// The canonical subset of a handshake that gets signed (ADR-0036,
/// "Decided: Handshake") — [`Hello`] wraps this with a signature; the
/// signature cannot cover itself, so this type excludes it
/// structurally, mirroring `VoteSigningPayloadV1`/
/// `TransactionSigningPayload`'s own shape exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelloSigningPayloadV1 {
    /// Structure version for this payload shape.
    pub hello_version: u16,
    /// The sender's own protocol version.
    pub protocol_version: ProtocolVersion,
    /// HNChain protocol lineage (ADR-0006).
    pub chain_id: u8,
    /// Network environment (ADR-0003).
    pub network_id: u16,
    /// The sender's own P2P node identity key (`KeyRole::NodeIdentity`).
    pub node_key: KeyDescriptor,
    /// Every channel the sender is willing to use.
    pub supported_channels: Vec<Channel>,
    /// Every message type the sender can decode and handle.
    pub supported_message_types: Vec<MessageType>,
}

impl HelloSigningPayloadV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// Appends this value's canonical HNCS bytes to `out`. Shared by
    /// [`HelloSigningPayloadV1::encode`] and [`Hello::encode`] so the
    /// two never drift apart.
    fn encode_into(&self, out: &mut Vec<u8>) -> NetworkResult<()> {
        write_u16(out, self.hello_version);
        write_u16(out, self.protocol_version.major());
        write_u16(out, self.protocol_version.minor());
        write_u16(out, self.protocol_version.patch());
        write_u8(out, self.chain_id);
        write_u16(out, self.network_id);
        encode_node_key(out, &self.node_key)?;

        // A hand-rolled, sorted-and-deduplicated set (matching
        // `hn_hncs::write_set`'s own canonical shape) rather than
        // `write_set` itself: `Channel`/`MessageType` values are
        // infallible to encode, but decoding a registry value can fail
        // with a domain-specific `NetworkError` `write_set`/`read_set`'s
        // `HncsResult`-typed closures cannot express — the same
        // justification `QuorumCertificate.aggregate_proof`/
        // `MultisigConfigV1.authorized_keys` already established for
        // the identical situation.
        let mut channels = self.supported_channels.clone();
        channels.sort();
        channels.dedup();
        write_channel_set(out, &channels)?;

        let mut message_types = self.supported_message_types.clone();
        message_types.sort();
        message_types.dedup();
        write_message_type_set(out, &message_types)?;

        Ok(())
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`HelloSigningPayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let payload = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(payload)
    }

    /// Decodes this value's fields from `decoder` without requiring it
    /// to be exhausted afterward. Shared by
    /// [`HelloSigningPayloadV1::decode`] and [`Hello::decode`], which
    /// has a `signature` field still to read.
    fn decode_from(decoder: &mut Decoder<'_>) -> NetworkResult<Self> {
        let hello_version = decoder.read_u16()?;
        if hello_version != HELLO_VERSION_1 {
            return Err(NetworkError::UnexpectedHandshakeEvent);
        }
        let major = decoder.read_u16()?;
        let minor = decoder.read_u16()?;
        let patch = decoder.read_u16()?;
        let protocol_version = ProtocolVersion::new(major, minor, patch);
        let chain_id = decoder.read_u8()?;
        let network_id = decoder.read_u16()?;
        let node_key = decode_node_key(decoder)?;
        let supported_channels = read_channel_set(decoder)?;
        let supported_message_types = read_message_type_set(decoder)?;

        Ok(Self {
            hello_version,
            protocol_version,
            chain_id,
            network_id,
            node_key,
            supported_channels,
            supported_message_types,
        })
    }

    /// Computes this payload's signing digest:
    /// `HASH_PROFILE_0x0001("hnchain.network.hello.v1", HNCS(HelloSigningPayloadV1))`.
    pub fn signing_digest(&self) -> NetworkResult<Digest> {
        Ok(hash_profile_0x0001(
            "hnchain.network.hello.v1",
            &self.encode()?,
        )?)
    }
}

/// A signed handshake message (ADR-0036, "Decided: Handshake") — one
/// message, each direction, carrying identity, protocol/chain/network
/// binding, and capabilities together (a deliberate merge of the RFC's
/// own separate `hello`/`capabilities` messages — see this ADR's own
/// "Rejected Options").
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hello {
    /// The signed content.
    pub payload: HelloSigningPayloadV1,
    /// The signature over `payload.signing_digest()`, proving
    /// `payload.node_key` possession.
    pub signature: SignatureEnvelope,
}

impl Hello {
    /// Encodes this value as canonical HNCS bytes: `payload` followed
    /// by the signature envelope.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        self.payload.encode_into(&mut out)?;
        self.signature.encode_into(&mut out)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`Hello::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let payload = HelloSigningPayloadV1::decode_from(&mut decoder)?;
        let signature = SignatureEnvelope::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(Self { payload, signature })
    }

    /// Verifies `signature` against `payload.signing_digest()` and
    /// `payload.node_key` — proves `node_key` possession only, not
    /// freshness (ADR-0036, "Explicitly Not Resolved": no
    /// replay/challenge protection in this pass).
    pub fn verify(&self) -> NetworkResult<()> {
        let digest = self.payload.signing_digest()?;
        self.signature.verify(&self.payload.node_key, &digest)?;
        Ok(())
    }
}

fn encode_node_key(out: &mut Vec<u8>, key: &KeyDescriptor) -> NetworkResult<()> {
    write_u16(out, key.algorithm_id());
    write_bytes(out, &key.public_key_bytes(), PUBLIC_KEY_MAX_LEN)?;
    Ok(())
}

fn decode_node_key(decoder: &mut Decoder<'_>) -> NetworkResult<KeyDescriptor> {
    let algorithm_id = decoder.read_u16()?;
    if algorithm_id != ED25519_ALGORITHM_ID {
        return Err(NetworkError::Identity(
            hn_crypto::IdentityError::UnsupportedAlgorithm {
                value: algorithm_id,
            },
        ));
    }
    let bytes = decoder.read_bytes(PUBLIC_KEY_MAX_LEN)?;
    let public_key: [u8; ED25519_PUBLIC_KEY_LEN] = bytes.try_into().map_err(|_| {
        NetworkError::Identity(hn_crypto::IdentityError::UnsupportedAlgorithm {
            value: algorithm_id,
        })
    })?;
    Ok(KeyDescriptor::from_public_key_bytes(
        KeyRole::NodeIdentity,
        public_key,
    )?)
}

fn write_channel_set(out: &mut Vec<u8>, channels: &[Channel]) -> NetworkResult<()> {
    hn_hncs::validate_count(channels.len(), MAX_CAPABILITY_COUNT)?;
    write_u32_count(out, channels.len());
    for channel in channels {
        write_u8(out, channel.as_u8());
    }
    Ok(())
}

fn read_channel_set(decoder: &mut Decoder<'_>) -> NetworkResult<Vec<Channel>> {
    let count = read_u32_count(decoder)?;
    hn_hncs::validate_count(count, MAX_CAPABILITY_COUNT)?;
    let mut channels = Vec::with_capacity(count);
    for _ in 0..count {
        channels.push(Channel::from_u8(decoder.read_u8()?)?);
    }
    Ok(channels)
}

fn write_message_type_set(out: &mut Vec<u8>, message_types: &[MessageType]) -> NetworkResult<()> {
    hn_hncs::validate_count(message_types.len(), MAX_CAPABILITY_COUNT)?;
    write_u32_count(out, message_types.len());
    for message_type in message_types {
        write_u16(out, message_type.as_u16());
    }
    Ok(())
}

fn read_message_type_set(decoder: &mut Decoder<'_>) -> NetworkResult<Vec<MessageType>> {
    let count = read_u32_count(decoder)?;
    hn_hncs::validate_count(count, MAX_CAPABILITY_COUNT)?;
    let mut message_types = Vec::with_capacity(count);
    for _ in 0..count {
        message_types.push(MessageType::from_u16(decoder.read_u16()?)?);
    }
    Ok(message_types)
}

fn write_u32_count(out: &mut Vec<u8>, count: usize) {
    hn_hncs::write_u32(out, count as u32);
}

fn read_u32_count(decoder: &mut Decoder<'_>) -> NetworkResult<usize> {
    Ok(decoder.read_u32()? as usize)
}

/// This node's own requirements for accepting a peer's [`Hello`]
/// (ADR-0036, "Decided: Handshake").
#[derive(Clone, Debug)]
pub struct HandshakeParams {
    /// This node's own protocol version (currently unchecked against
    /// the peer's beyond being present — no version-compatibility
    /// policy is decided yet).
    pub protocol_version: ProtocolVersion,
    /// This node's own `chain_id` — a peer's `Hello` must match
    /// exactly.
    pub chain_id: u8,
    /// This node's own `network_id` — a peer's `Hello` must match
    /// exactly.
    pub network_id: u16,
    /// Every channel this node requires a peer to support.
    pub required_channels: Vec<Channel>,
    /// Every message type this node requires a peer to support.
    pub required_message_types: Vec<MessageType>,
}

/// Where a handshake attempt currently stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakeStep {
    /// Neither sent nor evaluated a `Hello` yet.
    Idle,
    /// Sent this node's own `Hello`; still waiting to evaluate the
    /// peer's.
    SentHello,
    /// The peer's `Hello` was evaluated and accepted.
    Established,
    /// The peer's `Hello` was evaluated and rejected.
    Rejected,
}

/// An input to [`HandshakeState::apply`].
#[derive(Clone, Debug)]
pub enum HandshakeEvent {
    /// Send this node's own `Hello` to the peer.
    SendHello,
    /// A `Hello` was received from the peer.
    ReceiveHello(Hello),
}

/// What [`HandshakeState::apply`] decides the caller should do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandshakeAction {
    /// Send `hello` to the peer.
    Send(Hello),
    /// The peer's `Hello` was accepted — the handshake is complete.
    Accepted {
        /// The peer's own derived node identity.
        peer_id: Digest,
        /// The peer's accepted `Hello` payload.
        peer_hello: HelloSigningPayloadV1,
    },
    /// The peer's `Hello` was rejected, for `reason` — a legitimate
    /// protocol outcome (a real network will see mismatched chains,
    /// missing capabilities, and bad signatures), not a programming
    /// error, the same "legitimate outcome vs. inclusion-precondition
    /// failure" distinction `hn_state`'s own `apply_*` functions
    /// already draw between a `Failed` receipt and a hard `Err`.
    Rejected(NetworkError),
}

/// A pure handshake state machine (ADR-0036, "Decided: Handshake"),
/// mirroring `hn_consensus::ConsensusState`'s own shape exactly: no
/// I/O, no real timers, driven by explicit events, a deliberately
/// closed and total `apply` — every `(step, event)` pair not in the
/// transition table is `Err(NetworkError::UnexpectedHandshakeEvent)`,
/// never silently ignored.
pub struct HandshakeState {
    step: HandshakeStep,
    params: HandshakeParams,
    own_keypair: Ed25519KeyPair,
}

impl HandshakeState {
    /// Starts a fresh handshake attempt at [`HandshakeStep::Idle`].
    /// `own_keypair` must hold [`KeyRole::NodeIdentity`] — its private
    /// key signs this node's own outgoing [`Hello`].
    #[must_use]
    pub fn new(params: HandshakeParams, own_keypair: Ed25519KeyPair) -> Self {
        Self {
            step: HandshakeStep::Idle,
            params,
            own_keypair,
        }
    }

    /// This handshake's current step.
    #[must_use]
    pub fn step(&self) -> HandshakeStep {
        self.step
    }

    /// Applies `event`. See the type-level documentation for the
    /// total/closed-function contract.
    pub fn apply(&mut self, event: HandshakeEvent) -> NetworkResult<HandshakeAction> {
        match (self.step, event) {
            (HandshakeStep::Idle, HandshakeEvent::SendHello) => {
                let hello = self.build_hello()?;
                self.step = HandshakeStep::SentHello;
                Ok(HandshakeAction::Send(hello))
            }
            (
                HandshakeStep::Idle | HandshakeStep::SentHello,
                HandshakeEvent::ReceiveHello(hello),
            ) => match self.evaluate(&hello)? {
                Evaluation::Accepted {
                    peer_id,
                    peer_hello,
                } => {
                    self.step = HandshakeStep::Established;
                    Ok(HandshakeAction::Accepted {
                        peer_id,
                        peer_hello,
                    })
                }
                Evaluation::Rejected(reason) => {
                    self.step = HandshakeStep::Rejected;
                    Ok(HandshakeAction::Rejected(reason))
                }
            },
            (_, _) => Err(NetworkError::UnexpectedHandshakeEvent),
        }
    }

    fn build_hello(&self) -> NetworkResult<Hello> {
        let payload = HelloSigningPayloadV1 {
            hello_version: HELLO_VERSION_1,
            protocol_version: self.params.protocol_version,
            chain_id: self.params.chain_id,
            network_id: self.params.network_id,
            node_key: self.own_keypair.key_descriptor(),
            supported_channels: self.params.required_channels.clone(),
            supported_message_types: self.params.required_message_types.clone(),
        };
        let digest = payload.signing_digest()?;
        let signature = SignatureEnvelope {
            algorithm_id: payload.node_key.algorithm_id(),
            key_reference: None,
            signature: self.own_keypair.sign(&digest).to_vec(),
        };
        Ok(Hello { payload, signature })
    }

    fn evaluate(&self, hello: &Hello) -> NetworkResult<Evaluation> {
        if hello.verify().is_err() {
            return Ok(Evaluation::Rejected(NetworkError::Identity(
                hn_crypto::IdentityError::SignatureVerificationFailed,
            )));
        }
        if hello.payload.chain_id != self.params.chain_id {
            return Ok(Evaluation::Rejected(NetworkError::ChainMismatch {
                expected: self.params.chain_id,
                found: hello.payload.chain_id,
            }));
        }
        if hello.payload.network_id != self.params.network_id {
            return Ok(Evaluation::Rejected(NetworkError::NetworkMismatch {
                expected: self.params.network_id,
                found: hello.payload.network_id,
            }));
        }
        for required in &self.params.required_channels {
            if !hello.payload.supported_channels.contains(required) {
                return Ok(Evaluation::Rejected(NetworkError::MissingRequiredChannel {
                    channel: *required,
                }));
            }
        }
        for required in &self.params.required_message_types {
            if !hello.payload.supported_message_types.contains(required) {
                return Ok(Evaluation::Rejected(
                    NetworkError::MissingRequiredMessageType {
                        message_type: *required,
                    },
                ));
            }
        }

        Ok(Evaluation::Accepted {
            peer_id: peer_id(&hello.payload.node_key)?,
            peer_hello: hello.payload.clone(),
        })
    }
}

/// [`HandshakeState::evaluate`]'s own return shape — deliberately only
/// two variants (unlike [`HandshakeAction`]'s three), so `apply`'s own
/// match over it is exhaustive without a `Send` arm that could never
/// actually occur, needing no panicking fallback to satisfy the
/// compiler.
enum Evaluation {
    Accepted {
        peer_id: Digest,
        peer_hello: HelloSigningPayloadV1,
    },
    Rejected(NetworkError),
}

#[cfg(test)]
mod tests {
    use hn_core::ProtocolVersion;
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::{
        HandshakeAction, HandshakeEvent, HandshakeParams, HandshakeState, HandshakeStep, Hello,
        HelloSigningPayloadV1,
    };
    use crate::error::{NetworkError, NetworkResult};
    use crate::identity::peer_id;
    use crate::registry::{Channel, MessageType};

    fn params() -> HandshakeParams {
        HandshakeParams {
            protocol_version: ProtocolVersion::new(0, 1, 0),
            chain_id: 1,
            network_id: 1,
            required_channels: vec![Channel::Handshake, Channel::Transactions],
            required_message_types: vec![MessageType::Hello, MessageType::TransactionAnnounce],
        }
    }

    fn sample_payload(seed: u8) -> HelloSigningPayloadV1 {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [seed; 32]);
        HelloSigningPayloadV1 {
            hello_version: super::HELLO_VERSION_1,
            protocol_version: ProtocolVersion::new(0, 1, 0),
            chain_id: 1,
            network_id: 1,
            node_key: keypair.key_descriptor(),
            supported_channels: vec![Channel::Handshake, Channel::Transactions],
            supported_message_types: vec![MessageType::Hello, MessageType::TransactionAnnounce],
        }
    }

    fn signed(payload: HelloSigningPayloadV1, keypair: &Ed25519KeyPair) -> NetworkResult<Hello> {
        let digest = payload.signing_digest()?;
        let signature = hn_crypto::SignatureEnvelope {
            algorithm_id: payload.node_key.algorithm_id(),
            key_reference: None,
            signature: keypair.sign(&digest).to_vec(),
        };
        Ok(Hello { payload, signature })
    }

    #[test]
    fn signing_digest_matches_independent_oracle() -> NetworkResult<()> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x33; 32]);
        let payload = HelloSigningPayloadV1 {
            hello_version: super::HELLO_VERSION_1,
            protocol_version: ProtocolVersion::new(0, 1, 0),
            chain_id: 1,
            network_id: 1,
            node_key: keypair.key_descriptor(),
            supported_channels: vec![Channel::Transactions, Channel::Handshake],
            supported_message_types: vec![MessageType::Hello, MessageType::TransactionAnnounce],
        };
        let digest = payload.signing_digest()?;
        assert_eq!(
            hex(&digest),
            "ff35b95a7ef9a687f3e125db215d6ede34b089e04d07a4649a85fc2eaff2998d"
        );
        Ok(())
    }

    #[test]
    fn hello_round_trips_through_decode() -> NetworkResult<()> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x01; 32]);
        let hello = signed(sample_payload(0x01), &keypair)?;
        let decoded = Hello::decode(&hello.encode()?)?;
        assert_eq!(decoded, hello);
        Ok(())
    }

    #[test]
    fn encode_canonicalizes_capability_order() -> NetworkResult<()> {
        // Constructed out of order and with a duplicate; encode must
        // sort and deduplicate, decode must accept the canonical
        // result.
        let keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x04; 32]);
        let mut payload = sample_payload(0x04);
        payload.supported_channels = vec![
            Channel::Transactions,
            Channel::Handshake,
            Channel::Transactions,
        ];
        let hello = signed(payload, &keypair)?;

        let decoded = Hello::decode(&hello.encode()?)?;
        assert_eq!(
            decoded.payload.supported_channels,
            vec![Channel::Handshake, Channel::Transactions]
        );
        Ok(())
    }

    #[test]
    fn hello_verify_accepts_a_real_signature() -> NetworkResult<()> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x02; 32]);
        let hello = signed(sample_payload(0x02), &keypair)?;
        hello.verify()
    }

    #[test]
    fn hello_verify_rejects_a_tampered_payload() -> NetworkResult<()> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x03; 32]);
        let mut hello = signed(sample_payload(0x03), &keypair)?;
        hello.payload.network_id = 999;
        assert!(hello.verify().is_err());
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn full_handshake_accepts_a_matching_peer() -> NetworkResult<()> {
        let mut state = HandshakeState::new(
            params(),
            Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x10; 32]),
        );
        let action = state.apply(HandshakeEvent::SendHello)?;
        assert!(matches!(action, HandshakeAction::Send(_)));
        assert_eq!(state.step(), HandshakeStep::SentHello);

        let peer_keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x11; 32]);
        let peer_hello = signed(sample_payload(0x11), &peer_keypair)?;
        let expected_peer_id = peer_id(&peer_hello.payload.node_key)?;

        let action = state.apply(HandshakeEvent::ReceiveHello(peer_hello))?;
        assert!(matches!(
            &action,
            HandshakeAction::Accepted { peer_id, .. } if *peer_id == expected_peer_id
        ));
        assert_eq!(state.step(), HandshakeStep::Established);
        Ok(())
    }

    #[test]
    fn rejects_a_chain_mismatch() -> NetworkResult<()> {
        let mut state = HandshakeState::new(
            params(),
            Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x20; 32]),
        );
        let peer_keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x21; 32]);
        let mut payload = sample_payload(0x21);
        payload.chain_id = 2;
        let peer_hello = signed(payload, &peer_keypair)?;

        let action = state.apply(HandshakeEvent::ReceiveHello(peer_hello))?;
        assert!(matches!(
            action,
            HandshakeAction::Rejected(NetworkError::ChainMismatch {
                expected: 1,
                found: 2
            })
        ));
        assert_eq!(state.step(), HandshakeStep::Rejected);
        Ok(())
    }

    #[test]
    fn rejects_a_network_mismatch() -> NetworkResult<()> {
        let mut state = HandshakeState::new(
            params(),
            Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x30; 32]),
        );
        let peer_keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x31; 32]);
        let mut payload = sample_payload(0x31);
        payload.network_id = 2;
        let peer_hello = signed(payload, &peer_keypair)?;

        let action = state.apply(HandshakeEvent::ReceiveHello(peer_hello))?;
        assert!(matches!(
            action,
            HandshakeAction::Rejected(NetworkError::NetworkMismatch {
                expected: 1,
                found: 2
            })
        ));
        Ok(())
    }

    #[test]
    fn rejects_a_missing_required_channel() -> NetworkResult<()> {
        let mut state = HandshakeState::new(
            params(),
            Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x40; 32]),
        );
        let peer_keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x41; 32]);
        let mut payload = sample_payload(0x41);
        payload.supported_channels = vec![Channel::Handshake]; // missing Transactions
        let peer_hello = signed(payload, &peer_keypair)?;

        let action = state.apply(HandshakeEvent::ReceiveHello(peer_hello))?;
        assert!(matches!(
            action,
            HandshakeAction::Rejected(NetworkError::MissingRequiredChannel {
                channel: Channel::Transactions
            })
        ));
        Ok(())
    }

    #[test]
    fn rejects_a_bad_signature() -> NetworkResult<()> {
        let mut state = HandshakeState::new(
            params(),
            Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x50; 32]),
        );
        let peer_keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x51; 32]);
        let mut peer_hello = signed(sample_payload(0x51), &peer_keypair)?;
        peer_hello.payload.network_id = 1; // no-op, keep matching
        peer_hello.signature.signature[0] ^= 0xFF; // tamper

        let action = state.apply(HandshakeEvent::ReceiveHello(peer_hello))?;
        assert!(matches!(
            action,
            HandshakeAction::Rejected(NetworkError::Identity(_))
        ));
        Ok(())
    }

    #[test]
    fn rejects_an_out_of_step_event() -> NetworkResult<()> {
        let mut state = HandshakeState::new(
            params(),
            Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x60; 32]),
        );
        state.apply(HandshakeEvent::SendHello)?;
        // Sending a second Hello has no defined transition.
        assert_eq!(
            state.apply(HandshakeEvent::SendHello),
            Err(NetworkError::UnexpectedHandshakeEvent)
        );
        Ok(())
    }
}
