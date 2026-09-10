use hn_core::{BlockHeight, Epoch, Round};
use hn_crypto::{Digest, hash_profile_0x0001};
use hn_hncs::{Decoder, write_bytes, write_fixed_bytes, write_u8, write_u16, write_u64};

use crate::error::{StateError, StateResult};

/// `vote_version` for the current `VoteSigningPayloadV1`/`ConsensusVote`
/// shape (ADR-0012, "Vote Context Binding").
pub const VOTE_VERSION_1: u16 = 1;

/// `consensus_profile` identifying the Tendermint-style profile decided
/// in ADR-0009 (ADR-0012, "Decided: `consensus_profile` type"). A
/// single-value profile identifier, matching `tree_profile`
/// (ADR-0007) and `LIST_TREE_PROFILE_ID` (ADR-0008) — `0x00` is not
/// reserved here the way a per-object registry like `chain_id` reserves
/// it.
pub const CONSENSUS_PROFILE_TENDERMINT_V1: u16 = 1;

/// Maximum length, in bytes, of `vote_metadata`. An implementation
/// resource bound, not derived — same class as `MAX_ACCESS_LIST_ENTRIES`
/// (ADR-0006).
pub const MAX_VOTE_METADATA_LEN: usize = 256;

/// The `vote_type` registry (ADR-0012, "Decided: `vote_type` registry
/// for Tendermint-style BFT"), closed for this profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VoteType {
    /// The first of Tendermint's two voting stages.
    Prevote = 0x01,
    /// The second of Tendermint's two voting stages; a `precommit`
    /// quorum is what finality (ADR-0013) certifies.
    Precommit = 0x02,
}

impl VoteType {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::Prevote),
            0x02 => Ok(Self::Precommit),
            _ => Err(StateError::InvalidVoteType { value }),
        }
    }

    const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// The `target_type` registry (ADR-0012, "Decided: `target_type`
/// registry"), closed for this profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VoteTargetType {
    /// `target_hash` is a real `block_hash` (ADR-0008).
    Block = 0x01,
    /// `target_hash` is the all-zero digest — a timeout/no-progress
    /// vote (ADR-0009, "Timeout And View Change").
    Nil = 0x02,
}

impl VoteTargetType {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::Block),
            0x02 => Ok(Self::Nil),
            _ => Err(StateError::InvalidVoteTargetType { value }),
        }
    }

    const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// The canonical subset of a consensus vote that gets signed (ADR-0012,
/// "Vote Context Binding"). `ConsensusVote` (below) wraps this with a
/// signature; the signature cannot cover itself, so this type excludes
/// it structurally, not just by convention.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoteSigningPayloadV1 {
    /// Which of Tendermint's two voting stages this vote belongs to.
    pub vote_type: VoteType,
    /// HNChain protocol lineage (ADR-0006, "Chain And Network
    /// Binding").
    pub chain_id: u8,
    /// Network environment (ADR-0003, "Decision 4").
    pub network_id: u16,
    /// The consensus/validator-set epoch this vote was cast under —
    /// not `protocol_epoch` (ADR-0008/ADR-0022), which this vote type
    /// has no field for.
    pub epoch: Epoch,
    /// The block height this vote is for.
    pub height: BlockHeight,
    /// The round, within `height`, this vote is for. Resets to
    /// `Round::FIRST` at the start of every new height.
    pub round: Round,
    /// The active validator set's commitment (ADR-0010) — the same
    /// value as [`crate::consensus_root`].
    pub validator_set_commitment: Digest,
    /// The voting validator's stable identifier (ADR-0012, "Decided:
    /// `validator_id` width, not its exact derivation").
    pub validator_id: Digest,
    /// What this vote targets.
    pub target_type: VoteTargetType,
    /// The target's hash. Must be the all-zero digest when
    /// `target_type` is [`VoteTargetType::Nil`] — enforced on decode,
    /// not just by convention, so there is exactly one canonical
    /// encoding of a nil vote.
    pub target_hash: Digest,
    /// Bounded, profile-specific metadata (ADR-0012, "Vote Metadata").
    pub vote_metadata: Vec<u8>,
}

impl VoteSigningPayloadV1 {
    /// Encodes this value as canonical HNCS bytes (ADR-0012, "Vote
    /// Context Binding").
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// Appends this value's canonical HNCS bytes to `out`. Shared by
    /// [`VoteSigningPayloadV1::encode`] and [`ConsensusVote::encode`]
    /// so the two never drift apart.
    fn encode_into(&self, out: &mut Vec<u8>) -> StateResult<()> {
        write_u16(out, VOTE_VERSION_1);
        write_u16(out, CONSENSUS_PROFILE_TENDERMINT_V1);
        write_u8(out, self.vote_type.as_u8());
        write_u8(out, self.chain_id);
        write_u16(out, self.network_id);
        write_u64(out, self.epoch.get());
        write_u64(out, self.height.get());
        write_u64(out, self.round.get());
        write_fixed_bytes(out, &self.validator_set_commitment);
        write_fixed_bytes(out, &self.validator_id);
        write_u8(out, self.target_type.as_u8());
        write_fixed_bytes(out, &self.target_hash);
        write_bytes(out, &self.vote_metadata, MAX_VOTE_METADATA_LEN).map_err(StateError::Encoding)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`VoteSigningPayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let payload = Self::decode_from(&mut decoder)?;
        decoder.finish().map_err(StateError::Encoding)?;
        Ok(payload)
    }

    /// Decodes this value's fields from `decoder` without requiring the
    /// decoder to be exhausted afterward. Shared by
    /// [`VoteSigningPayloadV1::decode`] and [`ConsensusVote::decode`],
    /// which has a `signature` field still to read.
    fn decode_from(decoder: &mut Decoder<'_>) -> StateResult<Self> {
        let vote_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if vote_version != VOTE_VERSION_1 {
            return Err(StateError::UnsupportedVoteVersion {
                value: vote_version,
            });
        }
        let consensus_profile = decoder.read_u16().map_err(StateError::Encoding)?;
        if consensus_profile != CONSENSUS_PROFILE_TENDERMINT_V1 {
            return Err(StateError::UnsupportedConsensusProfile {
                value: consensus_profile,
            });
        }

        let vote_type = VoteType::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let chain_id = decoder.read_u8().map_err(StateError::Encoding)?;
        let network_id = decoder.read_u16().map_err(StateError::Encoding)?;
        let epoch = Epoch::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let height = BlockHeight::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let round = Round::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let validator_set_commitment = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let validator_id = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let target_type =
            VoteTargetType::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let target_hash = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let vote_metadata = decoder
            .read_bytes(MAX_VOTE_METADATA_LEN)
            .map_err(StateError::Encoding)?
            .to_vec();

        if target_type == VoteTargetType::Nil && target_hash != [0_u8; 32] {
            return Err(StateError::NonCanonicalNilTarget);
        }

        Ok(Self {
            vote_type,
            chain_id,
            network_id,
            epoch,
            height,
            round,
            validator_set_commitment,
            validator_id,
            target_type,
            target_hash,
            vote_metadata,
        })
    }

    /// Computes this payload's signing digest (ADR-0012, "Decided:
    /// vote signing digest mechanism"):
    /// `HASH_PROFILE_0x0001("hnchain.vote.signing.v1", HNCS(VoteSigningPayloadV1))`.
    pub fn signing_digest(&self) -> StateResult<Digest> {
        Ok(hash_profile_0x0001(
            "hnchain.vote.signing.v1",
            &self.encode()?,
        )?)
    }
}

/// A consensus vote (ADR-0012, "Vote Context Binding"): a signed
/// [`VoteSigningPayloadV1`]. `signature` is bounded raw bytes, not yet
/// a concrete `SignatureEnvelope` (ADR-0002) -- that container's own
/// canonical byte-level encoding has not been implemented in this
/// codebase yet, matching how `TransferPayloadV1`'s eventual
/// `TransactionEnvelope.signatures` field is similarly not yet a
/// concrete type anywhere in this project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusVote {
    /// The signed content.
    pub payload: VoteSigningPayloadV1,
    /// The signature over `payload.signing_digest()`.
    pub signature: Vec<u8>,
}

/// Maximum length, in bytes, of `ConsensusVote.signature`. Generous
/// headroom over Ed25519's 64-byte signatures (ADR-0002) for algorithm
/// agility, matching `hn_crypto::PUBLIC_KEY_MAX_LEN`'s reasoning.
pub const MAX_VOTE_SIGNATURE_LEN: usize = 256;

impl ConsensusVote {
    /// Encodes this value as canonical HNCS bytes: `payload` followed
    /// by the bounded `signature`.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        self.payload.encode_into(&mut out)?;
        write_bytes(&mut out, &self.signature, MAX_VOTE_SIGNATURE_LEN)
            .map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ConsensusVote::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let payload = VoteSigningPayloadV1::decode_from(&mut decoder)?;
        let signature = decoder
            .read_bytes(MAX_VOTE_SIGNATURE_LEN)
            .map_err(StateError::Encoding)?
            .to_vec();
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { payload, signature })
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, Round};

    use super::{ConsensusVote, StateError, VoteSigningPayloadV1, VoteTargetType, VoteType};
    use crate::error::StateResult;

    const VSC: [u8; 32] = [0x11; 32];
    const VID: [u8; 32] = [0x22; 32];
    const TH: [u8; 32] = [0x33; 32];

    fn prevote_block() -> VoteSigningPayloadV1 {
        VoteSigningPayloadV1 {
            vote_type: VoteType::Prevote,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(5),
            height: BlockHeight::new(1000),
            round: Round::new(0),
            validator_set_commitment: VSC,
            validator_id: VID,
            target_type: VoteTargetType::Block,
            target_hash: TH,
            vote_metadata: vec![],
        }
    }

    fn precommit_nil() -> VoteSigningPayloadV1 {
        VoteSigningPayloadV1 {
            vote_type: VoteType::Precommit,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(5),
            height: BlockHeight::new(1000),
            round: Round::new(1),
            validator_set_commitment: VSC,
            validator_id: VID,
            target_type: VoteTargetType::Nil,
            target_hash: [0; 32],
            vote_metadata: b"meta".to_vec(),
        }
    }

    #[test]
    fn encodes_prevote_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&prevote_block().encode()?),
            "01000100010101000500000000000000e80300000000000000000000000000001111111111111111111111111111111111111111111111111111111111111111222222222222222222222222222222222222222222222222222222222222222201333333333333333333333333333333333333333333333333333333333333333300000000"
        );
        Ok(())
    }

    #[test]
    fn encodes_nil_precommit_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&precommit_nil().encode()?),
            "01000100020101000500000000000000e803000000000000010000000000000011111111111111111111111111111111111111111111111111111111111111112222222222222222222222222222222222222222222222222222222222222222020000000000000000000000000000000000000000000000000000000000000000040000006d657461"
        );
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        for payload in [prevote_block(), precommit_nil()] {
            let decoded = VoteSigningPayloadV1::decode(&payload.encode()?)?;
            assert_eq!(decoded, payload);
        }
        Ok(())
    }

    #[test]
    fn rejects_nonzero_target_hash_on_nil_vote() -> StateResult<()> {
        let mut bad = precommit_nil();
        bad.target_hash = [0x01; 32];
        assert_eq!(
            VoteSigningPayloadV1::decode(&bad.encode()?),
            Err(StateError::NonCanonicalNilTarget)
        );
        Ok(())
    }

    #[test]
    fn rejects_unsupported_vote_version() -> StateResult<()> {
        let mut encoded = prevote_block().encode()?;
        encoded[0] = 0x02; // vote_version low byte, little-endian
        assert_eq!(
            VoteSigningPayloadV1::decode(&encoded),
            Err(StateError::UnsupportedVoteVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn encodes_consensus_vote_matching_independent_oracle() -> StateResult<()> {
        let vote = ConsensusVote {
            payload: prevote_block(),
            signature: vec![0x99; 64],
        };
        assert_eq!(
            hex(&vote.encode()?),
            "01000100010101000500000000000000e803000000000000000000000000000011111111111111111111111111111111111111111111111111111111111111112222222222222222222222222222222222222222222222222222222222222222013333333333333333333333333333333333333333333333333333333333333333000000004000000099999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999"
        );
        Ok(())
    }

    #[test]
    fn consensus_vote_round_trips_through_decode() -> StateResult<()> {
        let vote = ConsensusVote {
            payload: precommit_nil(),
            signature: vec![0xab; 64],
        };
        let decoded = ConsensusVote::decode(&vote.encode()?)?;
        assert_eq!(decoded, vote);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
