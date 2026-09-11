use hn_core::{BlockHeight, Epoch, Round};
use hn_crypto::{Digest, SignatureEnvelope, hash_profile_0x0001};
use hn_hncs::{
    Decoder, HncsError, validate_count, write_bytes, write_fixed_bytes, write_u8, write_u16,
    write_u32, write_u64, write_u128,
};

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
/// [`VoteSigningPayloadV1`]. `signature` is a concrete
/// [`hn_crypto::SignatureEnvelope`] (ADR-0002, "Decided:
/// `SignatureEnvelope` concrete field list") — verification still needs
/// an externally-resolved `KeyDescriptor` ([`SignatureEnvelope::verify`]
/// takes one rather than looking it up), so this crate does not verify
/// signatures itself, only encodes/decodes the envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusVote {
    /// The signed content.
    pub payload: VoteSigningPayloadV1,
    /// The signature over `payload.signing_digest()`.
    pub signature: SignatureEnvelope,
}

impl ConsensusVote {
    /// Encodes this value as canonical HNCS bytes: `payload` followed
    /// by the signature envelope.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        self.payload.encode_into(&mut out)?;
        self.signature
            .encode_into(&mut out)
            .map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ConsensusVote::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let payload = VoteSigningPayloadV1::decode_from(&mut decoder)?;
        let signature = SignatureEnvelope::decode_from(&mut decoder)
            .map_err(StateError::InvalidSignatureEnvelope)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { payload, signature })
    }
}

/// `qc_version` for the current `QuorumCertificate` shape (ADR-0012,
/// independent of `VOTE_VERSION_1` — ADR-0022's Nested Structure
/// Versions convention gives each structurally independent object its
/// own version field rather than sharing one across types).
pub const QC_VERSION_1: u16 = 1;

/// Maximum length, in bytes, of `QuorumCertificate.signer_commitment`.
/// An implementation resource bound picked ahead of ADR-0010's still-open
/// "maximum active set size, if any" — same relationship
/// `hn_core`/`hn-state`'s `OBJECT_ID_MAX_LEN` has with ADR-0003's final
/// address body length (ADR-0012, "signer commitment bit-level
/// encoding"). 1024 bytes covers up to 8192 validators, generous
/// headroom for a v0.1 active set; raise if a future active set size
/// decision needs more, never shrink silently.
pub const MAX_SIGNER_COMMITMENT_LEN: usize = 1024;

/// Maximum number of entries in `QuorumCertificate.aggregate_proof`.
/// Matches [`MAX_SIGNER_COMMITMENT_LEN`]'s 8192-validator headroom
/// (`1024 * 8` possible signer bits) — every set bit in
/// `signer_commitment` corresponds to exactly one entry here (ADR-0012,
/// "Decided: individual signatures with bitmap").
pub const MAX_QUORUM_SIGNATURES: usize = 8192;

/// A quorum certificate (ADR-0012): proof that sufficient voting power
/// signed the same target under the same validator set. `certificate_type`
/// reuses [`VoteType`]'s registry directly — ADR-0012's own text says this
/// "mirrors `vote_type`," the values are identical, so no second registry
/// is defined for the same two stages. `target_type`/`target_hash` reuse
/// [`VoteTargetType`] for the same reason: the registry is decided as
/// identical to the vote's own (ADR-0012, "Decided: `target_type`
/// registry").
///
/// `quorum_threshold` (present in ADR-0012's earliest conceptual sketch)
/// is deliberately not a field here: the threshold formula is fixed for
/// this profile (`signed_voting_power * 3 > total_voting_power * 2`), so
/// a stored field would duplicate what `total_voting_power` alone already
/// determines.
///
/// `aggregate_proof[i]` is the individual [`hn_crypto::SignatureEnvelope`]
/// of the `i`-th signer named by `signer_commitment`'s set bits, in
/// ascending bit-index order (ADR-0012, "Decided: signer commitment
/// bit-level encoding") — there is no cryptographic aggregation under the
/// decided scheme. Encoded as a hand-rolled `u32 count || elements`
/// sequence, not `hn_hncs::write_list`/`read_list`: each element's own
/// decode can fail with a domain-specific
/// [`hn_crypto::IdentityError`] (an unsupported `envelope_version`), which
/// those generic helpers' `HncsResult`-typed element closures cannot
/// express.
///
/// [`QuorumCertificate::decode`] checks only what is intrinsic to the
/// structure itself (matching [`VoteSigningPayloadV1::decode`]'s
/// `NonCanonicalNilTarget` check): version/profile support, the nil-target
/// invariant, `signed_voting_power <= total_voting_power`, and that
/// `aggregate_proof`'s entry count matches `signer_commitment`'s set-bit
/// count. Checks that need the active validator set for the referenced
/// epoch — signer eligibility, signer uniqueness against real
/// `validator_id`s, whether `signer_commitment`'s padding bits (beyond the
/// active set's real size) are zero, whether `total_voting_power` is
/// actually correct for that set, whether each signature verifies
/// (needs a resolved `KeyDescriptor` per signer, [`SignatureEnvelope::verify`]),
/// and whether the certificate meets quorum — are deliberately out of
/// scope here; they need an active-set query interface
/// ([`crate::active_set`]) wired to real storage-backed state, which this
/// codebase does not have yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuorumCertificate {
    /// Which of Tendermint's two voting stages this certificate proves
    /// quorum for.
    pub certificate_type: VoteType,
    /// HNChain protocol lineage (ADR-0006, "Chain And Network Binding").
    pub chain_id: u8,
    /// Network environment (ADR-0003, "Decision 4").
    pub network_id: u16,
    /// The consensus/validator-set epoch this certificate was formed
    /// under.
    pub epoch: Epoch,
    /// The block height this certificate is for.
    pub height: BlockHeight,
    /// The round, within `height`, this certificate is for.
    pub round: Round,
    /// The active validator set's commitment (ADR-0010) this
    /// certificate's signers and voting power totals are computed
    /// against.
    pub validator_set_commitment: Digest,
    /// What this certificate targets.
    pub target_type: VoteTargetType,
    /// The target's hash. Must be the all-zero digest when `target_type`
    /// is [`VoteTargetType::Nil`], mirroring
    /// [`VoteSigningPayloadV1::decode_from`]'s rule.
    pub target_hash: Digest,
    /// Total voting power of the active set this certificate was formed
    /// against (ADR-0010, "Decided: voting power integer type — `u128`").
    pub total_voting_power: u128,
    /// Voting power actually represented by `aggregate_proof`'s signers.
    pub signed_voting_power: u128,
    /// Bitmap over the active set, ascending `validator_id` order,
    /// identifying which validators signed (ADR-0012, "Decided: signer
    /// commitment bit-level encoding").
    pub signer_commitment: Vec<u8>,
    /// One individual signature envelope per set bit in
    /// `signer_commitment`, in the same ascending order.
    pub aggregate_proof: Vec<SignatureEnvelope>,
}

impl QuorumCertificate {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, QC_VERSION_1);
        write_u16(&mut out, CONSENSUS_PROFILE_TENDERMINT_V1);
        write_u8(&mut out, self.certificate_type.as_u8());
        write_u8(&mut out, self.chain_id);
        write_u16(&mut out, self.network_id);
        write_u64(&mut out, self.epoch.get());
        write_u64(&mut out, self.height.get());
        write_u64(&mut out, self.round.get());
        write_fixed_bytes(&mut out, &self.validator_set_commitment);
        write_u8(&mut out, self.target_type.as_u8());
        write_fixed_bytes(&mut out, &self.target_hash);
        write_u128(&mut out, self.total_voting_power);
        write_u128(&mut out, self.signed_voting_power);
        write_bytes(&mut out, &self.signer_commitment, MAX_SIGNER_COMMITMENT_LEN)
            .map_err(StateError::Encoding)?;

        // Hand-rolled `u32 count || elements`, not `write_list`: see the
        // type-level documentation for why (each element's decode can
        // fail with a domain-specific `IdentityError`, which
        // `write_list`/`read_list`'s `HncsResult`-typed closures cannot
        // express).
        validate_count(self.aggregate_proof.len(), MAX_QUORUM_SIGNATURES)
            .map_err(StateError::Encoding)?;
        let entry_count = u32::try_from(self.aggregate_proof.len()).map_err(|_| {
            StateError::Encoding(HncsError::LengthFieldOverflow {
                length: self.aggregate_proof.len(),
            })
        })?;
        write_u32(&mut out, entry_count);
        for envelope in &self.aggregate_proof {
            envelope
                .encode_into(&mut out)
                .map_err(StateError::Encoding)?;
        }

        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`QuorumCertificate::encode`]. See the type-level documentation for
    /// exactly which invariants are checked here versus deferred to a
    /// future active-set-aware verification step.
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let qc_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if qc_version != QC_VERSION_1 {
            return Err(StateError::UnsupportedQcVersion { value: qc_version });
        }
        let consensus_profile = decoder.read_u16().map_err(StateError::Encoding)?;
        if consensus_profile != CONSENSUS_PROFILE_TENDERMINT_V1 {
            return Err(StateError::UnsupportedConsensusProfile {
                value: consensus_profile,
            });
        }

        let certificate_type = VoteType::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let chain_id = decoder.read_u8().map_err(StateError::Encoding)?;
        let network_id = decoder.read_u16().map_err(StateError::Encoding)?;
        let epoch = Epoch::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let height = BlockHeight::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let round = Round::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let validator_set_commitment = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let target_type =
            VoteTargetType::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let target_hash = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;

        if target_type == VoteTargetType::Nil && target_hash != [0_u8; 32] {
            return Err(StateError::NonCanonicalNilTarget);
        }

        let total_voting_power = decoder.read_u128().map_err(StateError::Encoding)?;
        let signed_voting_power = decoder.read_u128().map_err(StateError::Encoding)?;
        if signed_voting_power > total_voting_power {
            return Err(StateError::SignedVotingPowerExceedsTotal);
        }

        let signer_commitment = decoder
            .read_bytes(MAX_SIGNER_COMMITMENT_LEN)
            .map_err(StateError::Encoding)?
            .to_vec();

        // Hand-rolled counterpart to encode()'s hand-rolled write — see
        // the type-level documentation for why this isn't `read_list`.
        let entry_count = decoder.read_u32().map_err(StateError::Encoding)? as usize;
        validate_count(entry_count, MAX_QUORUM_SIGNATURES).map_err(StateError::Encoding)?;
        let mut aggregate_proof = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            aggregate_proof.push(
                SignatureEnvelope::decode_from(&mut decoder)
                    .map_err(StateError::InvalidSignatureEnvelope)?,
            );
        }

        decoder.finish().map_err(StateError::Encoding)?;

        let signer_count: usize = signer_commitment
            .iter()
            .map(|byte| byte.count_ones() as usize)
            .sum();
        if signer_count != aggregate_proof.len() {
            return Err(StateError::SignerCountMismatch);
        }

        Ok(Self {
            certificate_type,
            chain_id,
            network_id,
            epoch,
            height,
            round,
            validator_set_commitment,
            target_type,
            target_hash,
            total_voting_power,
            signed_voting_power,
            signer_commitment,
            aggregate_proof,
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, Round};
    use hn_crypto::SignatureEnvelope;

    use super::{
        ConsensusVote, QuorumCertificate, StateError, VoteSigningPayloadV1, VoteTargetType,
        VoteType,
    };
    use crate::error::StateResult;

    fn envelope(byte: u8) -> SignatureEnvelope {
        SignatureEnvelope {
            algorithm_id: 1,
            signature: vec![byte; 64],
        }
    }

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
            signature: envelope(0x99),
        };
        assert_eq!(
            hex(&vote.encode()?),
            "01000100010101000500000000000000e80300000000000000000000000000001111111111111111111111111111111111111111111111111111111111111111222222222222222222222222222222222222222222222222222222222222222201333333333333333333333333333333333333333333333333333333333333333300000000010001004000000099999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999"
        );
        Ok(())
    }

    #[test]
    fn consensus_vote_round_trips_through_decode() -> StateResult<()> {
        let vote = ConsensusVote {
            payload: precommit_nil(),
            signature: envelope(0xab),
        };
        let decoded = ConsensusVote::decode(&vote.encode()?)?;
        assert_eq!(decoded, vote);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn qc_block_3signers() -> QuorumCertificate {
        QuorumCertificate {
            certificate_type: VoteType::Precommit,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(5),
            height: BlockHeight::new(1000),
            round: Round::new(0),
            validator_set_commitment: VSC,
            target_type: VoteTargetType::Block,
            target_hash: TH,
            total_voting_power: 100,
            signed_voting_power: 70,
            signer_commitment: vec![0x15], // bits 0, 2, 4 set
            aggregate_proof: vec![envelope(0xaa), envelope(0xbb), envelope(0xcc)],
        }
    }

    fn qc_nil_0signers() -> QuorumCertificate {
        QuorumCertificate {
            certificate_type: VoteType::Prevote,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(5),
            height: BlockHeight::new(1000),
            round: Round::new(1),
            validator_set_commitment: VSC,
            target_type: VoteTargetType::Nil,
            target_hash: [0; 32],
            total_voting_power: 100,
            signed_voting_power: 0,
            signer_commitment: vec![0x00],
            aggregate_proof: vec![],
        }
    }

    #[test]
    fn encodes_qc_block_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&qc_block_3signers().encode()?),
            "01000100020101000500000000000000e8030000000000000000000000000000111111111111111111111111111111111111111111111111111111111111111101333333333333333333333333333333333333333333333333333333333333333364000000000000000000000000000000460000000000000000000000000000000100000015030000000100010040000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0100010040000000bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb0100010040000000cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        Ok(())
    }

    #[test]
    fn encodes_qc_nil_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&qc_nil_0signers().encode()?),
            "01000100010101000500000000000000e803000000000000010000000000000011111111111111111111111111111111111111111111111111111111111111110200000000000000000000000000000000000000000000000000000000000000006400000000000000000000000000000000000000000000000000000000000000010000000000000000"
        );
        Ok(())
    }

    #[test]
    fn qc_round_trips_through_decode() -> StateResult<()> {
        for qc in [qc_block_3signers(), qc_nil_0signers()] {
            let decoded = QuorumCertificate::decode(&qc.encode()?)?;
            assert_eq!(decoded, qc);
        }
        Ok(())
    }

    #[test]
    fn rejects_unsupported_qc_version() -> StateResult<()> {
        let mut encoded = qc_block_3signers().encode()?;
        encoded[0] = 0x02; // qc_version low byte, little-endian
        assert_eq!(
            QuorumCertificate::decode(&encoded),
            Err(StateError::UnsupportedQcVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_nonzero_target_hash_on_nil_qc() -> StateResult<()> {
        let mut bad = qc_nil_0signers();
        bad.target_hash = [0x01; 32];
        assert_eq!(
            QuorumCertificate::decode(&bad.encode()?),
            Err(StateError::NonCanonicalNilTarget)
        );
        Ok(())
    }

    #[test]
    fn rejects_signed_voting_power_exceeding_total() -> StateResult<()> {
        let mut bad = qc_block_3signers();
        bad.signed_voting_power = bad.total_voting_power + 1;
        assert_eq!(
            QuorumCertificate::decode(&bad.encode()?),
            Err(StateError::SignedVotingPowerExceedsTotal)
        );
        Ok(())
    }

    #[test]
    fn rejects_signer_commitment_aggregate_proof_count_mismatch() -> StateResult<()> {
        let mut bad = qc_block_3signers();
        bad.aggregate_proof.pop(); // 3 signer bits set, only 2 signatures now
        assert_eq!(
            QuorumCertificate::decode(&bad.encode()?),
            Err(StateError::SignerCountMismatch)
        );
        Ok(())
    }
}
