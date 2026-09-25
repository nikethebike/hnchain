use std::collections::BTreeMap;

use hn_crypto::Digest;
use hn_hncs::{Decoder, HncsResult, write_fixed_bytes, write_list, write_string};

use crate::error::{NetworkError, NetworkResult};

/// Maximum length, in bytes, of `PeerAddressV1.network_address`. An
/// implementation resource bound, not derived — same class as
/// `GOVERNANCE_PROPOSAL_TITLE_MAX_LEN`.
pub const MAX_NETWORK_ADDRESS_LEN: usize = 256;

/// Maximum number of `PeerAddressV1` entries in one `PeerAnnounceV1`.
pub const MAX_ANNOUNCED_PEERS: usize = 64;

/// Maximum number of peers [`PeerTable`] retains. An implementation
/// resource bound (ADR-0036, "Security Considerations": "Unbounded
/// peer/message growth").
pub const MAX_KNOWN_PEERS: usize = 1024;

/// One peer's own identity plus where to reach it (ADR-0036, "Decided:
/// Discovery"). Deliberately transport-agnostic: `network_address` is a
/// bounded opaque string, not a typed `host:port` — no transport is
/// decided yet (ADR-0036's own "Explicitly Not Resolved").
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PeerAddressV1 {
    /// The peer's own [`crate::identity::peer_id`].
    pub peer_id: Digest,
    /// An opaque, transport-specific address string (for example,
    /// `"203.0.113.7:26656"` for a future TCP profile) — this crate
    /// does not interpret it.
    pub network_address: String,
}

impl PeerAddressV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// `HncsResult`-typed, not `NetworkResult`: encoding this value can
    /// only ever fail with a byte-framing error (the bounded string),
    /// never a domain-specific one, so it composes directly with
    /// `hn_hncs::write_list`'s own `HncsResult`-typed element closures
    /// — mirroring `hn_state`'s own `encode_key_descriptor`'s exact
    /// reasoning for the identical situation.
    fn encode_into(&self, out: &mut Vec<u8>) -> HncsResult<()> {
        write_fixed_bytes(out, &self.peer_id);
        write_string(out, &self.network_address, MAX_NETWORK_ADDRESS_LEN)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`PeerAddressV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }

    /// `HncsResult`-typed counterpart to [`PeerAddressV1::encode_into`],
    /// for the same reason.
    fn decode_from(decoder: &mut Decoder<'_>) -> HncsResult<Self> {
        let peer_id = decoder.read_fixed_bytes::<32>()?;
        let network_address = decoder.read_string(MAX_NETWORK_ADDRESS_LEN)?.to_string();
        Ok(Self {
            peer_id,
            network_address,
        })
    }
}

/// "Here are peers I know about" (ADR-0018, "Gossip Announcements";
/// ADR-0036, "Decided: Discovery"). `PeerAddressV1::decode` can only
/// fail with an ordinary `HncsError`, so this uses
/// `hn_hncs::write_list`/`read_list` directly, unlike the
/// `Vec<TransactionEnvelope>`-carrying propagation payloads
/// (`crate::propagation`), whose element decode can fail with a
/// domain-specific `hn_state::StateError`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerAnnounceV1 {
    /// The announced peers, bounded by [`MAX_ANNOUNCED_PEERS`].
    pub peers: Vec<PeerAddressV1>,
}

impl PeerAnnounceV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        write_list(&mut out, &self.peers, MAX_ANNOUNCED_PEERS, |out, peer| {
            peer.encode_into(out)
        })?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`PeerAnnounceV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let peers = decoder.read_list(MAX_ANNOUNCED_PEERS, PeerAddressV1::decode_from)?;
        decoder.finish()?;
        Ok(Self { peers })
    }
}

/// "Tell me what peers you know about" (ADR-0018, "Gossip
/// Announcements") — no payload fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerRequestV1;

impl PeerRequestV1 {
    /// Encodes this value as canonical HNCS bytes (empty — there is
    /// nothing to carry).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Decodes canonical HNCS bytes produced by
    /// [`PeerRequestV1::encode`]. Rejects any non-empty input
    /// ([`NetworkError::UnexpectedPeerRequestPayload`]) — a defined
    /// message with no fields getting unexpected bytes is worth
    /// rejecting cheaply, per ADR-0018's own "Cheap Rejection"
    /// philosophy, rather than silently ignoring the extra bytes.
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        if bytes.is_empty() {
            Ok(Self)
        } else {
            Err(NetworkError::UnexpectedPeerRequestPayload)
        }
    }
}

/// A bounded, in-memory table of known peers (ADR-0036, "Decided:
/// Discovery") — pure data structure, no I/O, no scoring, no eviction
/// policy beyond the flat [`MAX_KNOWN_PEERS`] capacity.
#[derive(Debug, Default)]
pub struct PeerTable {
    own_peer_id: Option<Digest>,
    peers: BTreeMap<Digest, PeerAddressV1>,
}

impl PeerTable {
    /// An empty table, optionally aware of this node's own `peer_id`
    /// (so [`PeerTable::handle_peer_announce`] can drop self-entries).
    #[must_use]
    pub fn new(own_peer_id: Option<Digest>) -> Self {
        Self {
            own_peer_id,
            peers: BTreeMap::new(),
        }
    }

    /// Seeds a table from a static bootstrap list (ADR-0036, "Decided:
    /// Discovery").
    #[must_use]
    pub fn with_bootstrap(own_peer_id: Option<Digest>, bootstrap: Vec<PeerAddressV1>) -> Self {
        let mut table = Self::new(own_peer_id);
        for peer in bootstrap {
            table.insert(peer);
        }
        table
    }

    /// How many peers this table currently holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether this table holds no peers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Whether `peer_id` is already known.
    #[must_use]
    pub fn contains(&self, peer_id: &Digest) -> bool {
        self.peers.contains_key(peer_id)
    }

    fn insert(&mut self, peer: PeerAddressV1) -> bool {
        if Some(peer.peer_id) == self.own_peer_id {
            return false;
        }
        if self.peers.len() >= MAX_KNOWN_PEERS && !self.peers.contains_key(&peer.peer_id) {
            return false;
        }
        self.peers.insert(peer.peer_id, peer).is_none()
    }

    /// Merges `announce`'s peers into this table (dropping self-entries
    /// and anything past [`MAX_KNOWN_PEERS`]). Returns how many entries
    /// were genuinely new — not blindly trusting everything a peer
    /// claims, but not verifying reachability either (ADR-0036's own
    /// "Security Considerations": connecting to a bad address wastes a
    /// dial attempt, nothing worse).
    pub fn handle_peer_announce(&mut self, announce: PeerAnnounceV1) -> usize {
        announce
            .peers
            .into_iter()
            .filter(|peer| self.insert(peer.clone()))
            .count()
    }

    /// Builds a response to a [`PeerRequestV1`]: up to `limit` known
    /// peers.
    #[must_use]
    pub fn build_peer_announce(&self, limit: usize) -> PeerAnnounceV1 {
        PeerAnnounceV1 {
            peers: self.peers.values().take(limit).cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PeerAddressV1, PeerAnnounceV1, PeerRequestV1, PeerTable};
    use crate::error::{NetworkError, NetworkResult};

    fn address(byte: u8, network_address: &str) -> PeerAddressV1 {
        PeerAddressV1 {
            peer_id: [byte; 32],
            network_address: network_address.to_string(),
        }
    }

    #[test]
    fn peer_address_round_trips_through_decode() -> NetworkResult<()> {
        let peer = address(0x01, "203.0.113.7:26656");
        let decoded = PeerAddressV1::decode(&peer.encode()?)?;
        assert_eq!(decoded, peer);
        Ok(())
    }

    #[test]
    fn peer_announce_round_trips_through_decode() -> NetworkResult<()> {
        let announce = PeerAnnounceV1 {
            peers: vec![address(0x01, "a"), address(0x02, "b")],
        };
        let decoded = PeerAnnounceV1::decode(&announce.encode()?)?;
        assert_eq!(decoded, announce);
        Ok(())
    }

    #[test]
    fn peer_request_round_trips_through_decode() -> NetworkResult<()> {
        let request = PeerRequestV1;
        let decoded = PeerRequestV1::decode(&request.encode())?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn peer_request_rejects_a_non_empty_payload() {
        assert_eq!(
            PeerRequestV1::decode(b"unexpected"),
            Err(NetworkError::UnexpectedPeerRequestPayload)
        );
    }

    #[test]
    fn with_bootstrap_seeds_the_table() {
        let table = PeerTable::with_bootstrap(None, vec![address(0x01, "a"), address(0x02, "b")]);
        assert_eq!(table.len(), 2);
        assert!(table.contains(&[0x01; 32]));
        assert!(table.contains(&[0x02; 32]));
    }

    #[test]
    fn handle_peer_announce_merges_new_entries_and_counts_them() {
        let mut table = PeerTable::new(None);
        let announce = PeerAnnounceV1 {
            peers: vec![address(0x01, "a"), address(0x02, "b")],
        };
        assert_eq!(table.handle_peer_announce(announce), 2);
        assert_eq!(table.len(), 2);

        // Re-announcing the same peer (with a new address) is not "new".
        let re_announce = PeerAnnounceV1 {
            peers: vec![address(0x01, "a-updated")],
        };
        assert_eq!(table.handle_peer_announce(re_announce), 0);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn handle_peer_announce_drops_a_self_entry() {
        let own_peer_id = [0x99; 32];
        let mut table = PeerTable::new(Some(own_peer_id));
        let announce = PeerAnnounceV1 {
            peers: vec![address(0x99, "self"), address(0x01, "a")],
        };
        assert_eq!(table.handle_peer_announce(announce), 1);
        assert!(!table.contains(&own_peer_id));
        assert!(table.contains(&[0x01; 32]));
    }

    #[test]
    fn build_peer_announce_respects_the_limit() {
        let table = PeerTable::with_bootstrap(
            None,
            vec![address(0x01, "a"), address(0x02, "b"), address(0x03, "c")],
        );
        let announce = table.build_peer_announce(2);
        assert_eq!(announce.peers.len(), 2);
    }

    #[test]
    fn an_empty_table_is_empty() {
        let table = PeerTable::new(None);
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }
}
