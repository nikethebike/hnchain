use hn_core::{BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};
use hn_crypto::Digest;
use hn_hncs::{Decoder, write_fixed_bytes, write_u8, write_u16, write_u64};

use crate::block_hash::block_hash;
use crate::error::{StateError, StateResult};

/// `header_version` for the current [`BlockHeader`] shape (ADR-0008,
/// "Versioned Block Envelope").
pub const HEADER_VERSION_1: u16 = 1;

/// The canonical block header (ADR-0008, "Decision" — `BlockHeader`),
/// assembled here for the first time as a concrete type: every field's
/// own wire shape was already decided, piece by piece, across this
/// project's own ADR-0008/ADR-0010/ADR-0012/ADR-0013/ADR-0015/ADR-0038
/// passes — this type does not decide anything new, it is the first
/// place all of those decisions are actually assembled together.
///
/// Field order matches ADR-0008's own conceptual `BlockHeader` listing
/// exactly, in the absence of any other decided order.
///
/// `epoch`/`round` reuse [`hn_core::Epoch`]/[`hn_core::Round`] directly
/// — the same consensus-protocol types `hn_state::VoteSigningPayloadV1`/
/// `QuorumCertificate` already use, not independently redecided here.
/// `protocol_epoch` reuses [`hn_core::ProtocolEpoch`], already built
/// specifically "ADR-0008, `BlockHeader.protocol_epoch`" per its own
/// doc comment. `timestamp` reuses [`hn_core::UnixTimeMillis`] — this
/// project's own real timestamp type, not a bare `u64` (ADR-0038's own
/// `GenesisManifest.genesis_time` predates this type's use here and
/// stays a plain `u64` of its own, unaffected by this choice — a
/// distinct, already-shipped decision this type does not revisit).
///
/// `parent_block_hash`/`proposer`/every `_root`/`_hash` field is a
/// [`Digest`] — this type stores whatever value the caller computed
/// (via `hn_state::{block_hash, consensus_root, evidence_digest,
/// extra_data_hash, protocol_parameters_placeholder_hash,
/// list_merkle_root, list_empty_root}`, `compute_state_root`, or
/// genesis's own already-decided values); it does not recompute or
/// validate any of them itself — the same "assembles already-verified
/// input, does not re-derive it" boundary this whole codebase already
/// draws between layers (for example
/// `hn_consensus::ConsensusState::apply` trusting its own caller).
/// Genesis-time values for a subset of these fields are already decided
/// (`state_root`/ADR-0038, `events_root`/`timestamp`/
/// `protocol_parameters_hash`/ADR-0008) — this type does not itself
/// decide `parent_block_hash`/`proposer`'s own still-open genesis
/// semantics, or wire this struct into genesis loading; that remains
/// future work, named rather than silently assumed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockHeader {
    /// Structure version for this header shape.
    pub header_version: u16,
    /// HNChain protocol lineage (ADR-0006, "Chain And Network
    /// Binding").
    pub chain_id: u8,
    /// Network environment (ADR-0003).
    pub network_id: u16,
    /// This block's position in the chain.
    pub height: BlockHeight,
    /// The consensus round this block was finalized at.
    pub round: Round,
    /// The consensus-protocol validator-set epoch (ADR-0008, "Height,
    /// Round, And Epoch") — distinct from `protocol_epoch`.
    pub epoch: Epoch,
    /// Which set of ADR-defined structure-version profiles is active
    /// at this height (ADR-0008, "Protocol Epoch"; ADR-0022).
    pub protocol_epoch: ProtocolEpoch,
    /// The canonical parent header's own `block_hash`. Genesis's own
    /// value here remains an open decision (ADR-0008, "Parent Link";
    /// `docs/specs/core/block-format.md`, "Genesis parent semantics
    /// are open") — this type accepts whatever the caller supplies.
    pub parent_block_hash: Digest,
    /// The proposing validator or system authority's own
    /// `address_body` (ADR-0008, "Proposer").
    pub proposer: Digest,
    /// This block's timestamp.
    pub timestamp: UnixTimeMillis,
    /// `hn-list-merkle-v1` commitment over each transaction's `tx_id`,
    /// in block order (ADR-0008, "Transactions Root").
    pub transactions_root: Digest,
    /// The post-execution state commitment (ADR-0007).
    pub state_root: Digest,
    /// `hn-list-merkle-v1` commitment over each receipt's digest, in
    /// the same order as `transactions_root` (ADR-0008, "Receipts
    /// Root").
    pub receipts_root: Digest,
    /// Commitment to consensus-visible events (ADR-0008, "Events
    /// Root") — real content gated on HNVM; `hn_state::list_empty_root`
    /// for any block producing none, genesis included.
    pub events_root: Digest,
    /// The active validator set's `validator_set_commitment`
    /// (ADR-0010, "Validator Set Commitment"; ADR-0008, "Consensus
    /// Root").
    pub consensus_root: Digest,
    /// `hn-list-merkle-v1` commitment over included Byzantine evidence,
    /// sorted by ascending `evidence_hash` (ADR-0008, "Evidence Root";
    /// ADR-0015).
    pub evidence_root: Digest,
    /// Commitment to the active protocol parameter set (ADR-0008,
    /// "Protocol Parameters Hash") —
    /// `hn_state::protocol_parameters_placeholder_hash` until a real
    /// adjustable parameter and commitment format both exist.
    pub protocol_parameters_hash: Digest,
    /// Commitment to `BlockBody.extra_data` (ADR-0008, "Decided: Extra
    /// Data Format").
    pub extra_data_hash: Digest,
}

impl BlockHeader {
    /// Encodes this header as canonical HNCS bytes, in the exact field
    /// order ADR-0008's own conceptual `BlockHeader` listing gives.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, self.header_version);
        write_u8(&mut out, self.chain_id);
        write_u16(&mut out, self.network_id);
        write_u64(&mut out, self.height.get());
        write_u64(&mut out, self.round.get());
        write_u64(&mut out, self.epoch.get());
        write_u64(&mut out, self.protocol_epoch.get());
        write_fixed_bytes(&mut out, &self.parent_block_hash);
        write_fixed_bytes(&mut out, &self.proposer);
        write_u64(&mut out, self.timestamp.as_millis());
        write_fixed_bytes(&mut out, &self.transactions_root);
        write_fixed_bytes(&mut out, &self.state_root);
        write_fixed_bytes(&mut out, &self.receipts_root);
        write_fixed_bytes(&mut out, &self.events_root);
        write_fixed_bytes(&mut out, &self.consensus_root);
        write_fixed_bytes(&mut out, &self.evidence_root);
        write_fixed_bytes(&mut out, &self.protocol_parameters_hash);
        write_fixed_bytes(&mut out, &self.extra_data_hash);
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`BlockHeader::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let header_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if header_version != HEADER_VERSION_1 {
            return Err(StateError::UnsupportedBlockHeaderVersion {
                value: header_version,
            });
        }
        let chain_id = decoder.read_u8().map_err(StateError::Encoding)?;
        let network_id = decoder.read_u16().map_err(StateError::Encoding)?;
        let height = BlockHeight::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let round = Round::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let epoch = Epoch::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let protocol_epoch = ProtocolEpoch::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let parent_block_hash = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let proposer = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let timestamp =
            UnixTimeMillis::from_millis(decoder.read_u64().map_err(StateError::Encoding)?);
        let transactions_root = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let state_root = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let receipts_root = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let events_root = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let consensus_root = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let evidence_root = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let protocol_parameters_hash = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let extra_data_hash = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;

        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            header_version,
            chain_id,
            network_id,
            height,
            round,
            epoch,
            protocol_epoch,
            parent_block_hash,
            proposer,
            timestamp,
            transactions_root,
            state_root,
            receipts_root,
            events_root,
            consensus_root,
            evidence_root,
            protocol_parameters_hash,
            extra_data_hash,
        })
    }

    /// This header's own `block_hash` (ADR-0008, "Header Hash"):
    /// `HASH_PROFILE_0x0001("hnchain.block.header.v1", HNCS(BlockHeader))`.
    pub fn block_hash(&self) -> StateResult<Digest> {
        block_hash(&self.encode()?)
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};

    use super::{BlockHeader, HEADER_VERSION_1};
    use crate::error::{StateError, StateResult};

    fn sample() -> BlockHeader {
        BlockHeader {
            header_version: HEADER_VERSION_1,
            chain_id: 1,
            network_id: 1,
            height: BlockHeight::new(42),
            round: Round::new(0),
            epoch: Epoch::new(5),
            protocol_epoch: ProtocolEpoch::new(0),
            parent_block_hash: [0x01; 32],
            proposer: [0x02; 32],
            timestamp: UnixTimeMillis::from_millis(1_758_758_400_000),
            transactions_root: [0x03; 32],
            state_root: [0x04; 32],
            receipts_root: [0x05; 32],
            events_root: [0x06; 32],
            consensus_root: [0x07; 32],
            evidence_root: [0x08; 32],
            protocol_parameters_hash: [0x09; 32],
            extra_data_hash: [0x0a; 32],
        }
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let header = sample();
        let decoded = BlockHeader::decode(&header.encode()?)?;
        assert_eq!(decoded, header);
        Ok(())
    }

    #[test]
    fn encode_matches_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&sample().encode()?),
            "01000101002a000000000000000000000000000000050000000000000000000000000000000101010101010101010101010101010101010101010101010101010101010101020202020202020202020202020202020202020202020202020202020202020200702b7e9901000003030303030303030303030303030303030303030303030303030303030303030404040404040404040404040404040404040404040404040404040404040404050505050505050505050505050505050505050505050505050505050505050506060606060606060606060606060606060606060606060606060606060606060707070707070707070707070707070707070707070707070707070707070707080808080808080808080808080808080808080808080808080808080808080809090909090909090909090909090909090909090909090909090909090909090a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"
        );
        Ok(())
    }

    #[test]
    fn block_hash_matches_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&sample().block_hash()?),
            "1af9155b05e634e5d5a240c39e584f85498b9aa50c0d3c8b0f2dfdffbcc36a57"
        );
        Ok(())
    }

    #[test]
    fn rejects_unsupported_header_version() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded[0] = 0x02; // header_version low byte, little-endian
        assert_eq!(
            BlockHeader::decode(&encoded),
            Err(StateError::UnsupportedBlockHeaderVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded.push(0xFF);
        assert!(BlockHeader::decode(&encoded).is_err());
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
