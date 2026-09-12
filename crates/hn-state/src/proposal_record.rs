use hn_core::BlockHeight;
use hn_crypto::Digest;
use hn_hncs::{
    Decoder, write_fixed_bytes, write_string, write_u8, write_u16, write_u64, write_u128,
};

use crate::error::{StateError, StateResult};
use crate::governance_payload::GOVERNANCE_PROPOSAL_TITLE_MAX_LEN;

/// `proposal_version` for the current `ProposalRecordV1` shape
/// (ADR-0025, "Decided: State Shape").
pub const PROPOSAL_RECORD_VERSION_1: u16 = 1;

/// The `status` registry (ADR-0025, "Decided: Proposal Outcome
/// States"), closed for this profile. `0x00` is reserved, matching
/// every other closed registry in this project.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ProposalStatus {
    /// The voting window is still open.
    Voting = 0x01,
    /// Quorum was met and a `for` majority was reached in both
    /// chambers.
    Passed = 0x02,
    /// Quorum was met in both chambers, but the `for`/`against`
    /// majority failed in at least one.
    Rejected = 0x03,
    /// Quorum was not met in at least one chamber by the voting
    /// window's close.
    Expired = 0x04,
}

impl ProposalStatus {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::Voting),
            0x02 => Ok(Self::Passed),
            0x03 => Ok(Self::Rejected),
            0x04 => Ok(Self::Expired),
            _ => Err(StateError::InvalidProposalStatus { value }),
        }
    }

    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// A governance proposal (ADR-0025, "Decided: State Shape").
///
/// Signaling-only ("Decided: Signaling Only"): this record is a
/// canonical, verifiable statement of what both chambers decided, not
/// an executable instruction — nothing in this crate or elsewhere
/// reads `status = Passed` and changes any other behavior because of
/// it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposalRecordV1 {
    /// This proposal's own id (ADR-0025, "Decided: `proposal_id`
    /// Derivation"). Stored here despite also being this record's own
    /// state key, the same redundancy `ValidatorRecordV1.validator_id`
    /// already accepts: a reader cannot invert a one-way state-key
    /// hash back to the id it committed to.
    pub proposal_id: Digest,
    /// The proposing validator's own `address_body` (ADR-0025,
    /// "Decided: Who May Propose" — must have been `Active` at
    /// creation time; this record does not re-verify that after the
    /// fact).
    pub proposer: Digest,
    /// A short, bounded, durable identifying title.
    pub title: String,
    /// A document commitment to the full proposal text, off-chain.
    pub content_hash: Digest,
    /// The height at which this proposal was created and its chamber
    /// snapshots taken.
    pub created_at_height: BlockHeight,
    /// The height at which the voting window closes.
    pub voting_ends_at_height: BlockHeight,
    /// This proposal's current outcome status.
    pub status: ProposalStatus,
    /// Count of `Active` validators (one vote each) that voted `for`
    /// in the validator chamber.
    pub validator_chamber_for: u128,
    /// Count that voted `against`.
    pub validator_chamber_against: u128,
    /// Count that voted `abstain`.
    pub validator_chamber_abstain: u128,
    /// Summed `bonded_stake` weight that voted `for` in the staker
    /// chamber.
    pub staker_chamber_for: u128,
    /// Summed weight that voted `against`.
    pub staker_chamber_against: u128,
    /// Summed weight that voted `abstain`.
    pub staker_chamber_abstain: u128,
    /// The validator chamber's total weight (count of `Active`
    /// validators), snapshotted at `created_at_height` and held fixed
    /// for this proposal's own lifetime (ADR-0025, "Snapshot
    /// Determinism").
    pub validator_chamber_total_weight: u128,
    /// The staker chamber's total weight (summed `bonded_stake`),
    /// snapshotted the same way.
    pub staker_chamber_total_weight: u128,
}

impl ProposalRecordV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, PROPOSAL_RECORD_VERSION_1);
        write_fixed_bytes(&mut out, &self.proposal_id);
        write_fixed_bytes(&mut out, &self.proposer);
        write_string(&mut out, &self.title, GOVERNANCE_PROPOSAL_TITLE_MAX_LEN)
            .map_err(StateError::Encoding)?;
        write_fixed_bytes(&mut out, &self.content_hash);
        write_u64(&mut out, self.created_at_height.get());
        write_u64(&mut out, self.voting_ends_at_height.get());
        write_u8(&mut out, self.status.as_u8());
        write_u128(&mut out, self.validator_chamber_for);
        write_u128(&mut out, self.validator_chamber_against);
        write_u128(&mut out, self.validator_chamber_abstain);
        write_u128(&mut out, self.staker_chamber_for);
        write_u128(&mut out, self.staker_chamber_against);
        write_u128(&mut out, self.staker_chamber_abstain);
        write_u128(&mut out, self.validator_chamber_total_weight);
        write_u128(&mut out, self.staker_chamber_total_weight);
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ProposalRecordV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let proposal_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if proposal_version != PROPOSAL_RECORD_VERSION_1 {
            return Err(StateError::UnsupportedProposalRecordVersion {
                value: proposal_version,
            });
        }

        let proposal_id = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let proposer = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let title = decoder
            .read_string(GOVERNANCE_PROPOSAL_TITLE_MAX_LEN)
            .map_err(StateError::Encoding)?
            .to_string();
        let content_hash = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let created_at_height = BlockHeight::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let voting_ends_at_height =
            BlockHeight::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let status = ProposalStatus::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let validator_chamber_for = decoder.read_u128().map_err(StateError::Encoding)?;
        let validator_chamber_against = decoder.read_u128().map_err(StateError::Encoding)?;
        let validator_chamber_abstain = decoder.read_u128().map_err(StateError::Encoding)?;
        let staker_chamber_for = decoder.read_u128().map_err(StateError::Encoding)?;
        let staker_chamber_against = decoder.read_u128().map_err(StateError::Encoding)?;
        let staker_chamber_abstain = decoder.read_u128().map_err(StateError::Encoding)?;
        let validator_chamber_total_weight = decoder.read_u128().map_err(StateError::Encoding)?;
        let staker_chamber_total_weight = decoder.read_u128().map_err(StateError::Encoding)?;

        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            proposal_id,
            proposer,
            title,
            content_hash,
            created_at_height,
            voting_ends_at_height,
            status,
            validator_chamber_for,
            validator_chamber_against,
            validator_chamber_abstain,
            staker_chamber_for,
            staker_chamber_against,
            staker_chamber_abstain,
            validator_chamber_total_weight,
            staker_chamber_total_weight,
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_core::BlockHeight;

    use super::{ProposalRecordV1, ProposalStatus, StateError};
    use crate::error::StateResult;

    const PROPOSAL_ID: [u8; 32] = [0x44; 32];
    const PROPOSER: [u8; 32] = [0x55; 32];
    const CONTENT_HASH: [u8; 32] = [0x66; 32];

    fn sample() -> ProposalRecordV1 {
        ProposalRecordV1 {
            proposal_id: PROPOSAL_ID,
            proposer: PROPOSER,
            title: "Raise MAX_ACTIVE_SET_SIZE".to_string(),
            content_hash: CONTENT_HASH,
            created_at_height: BlockHeight::new(1_000),
            voting_ends_at_height: BlockHeight::new(1_500),
            status: ProposalStatus::Voting,
            validator_chamber_for: 10,
            validator_chamber_against: 2,
            validator_chamber_abstain: 1,
            staker_chamber_for: 500_000,
            staker_chamber_against: 25_000,
            staker_chamber_abstain: 0,
            validator_chamber_total_weight: 20,
            staker_chamber_total_weight: 1_000_000,
        }
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let record = sample();
        let decoded = ProposalRecordV1::decode(&record.encode()?)?;
        assert_eq!(decoded, record);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_record_version() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded[0] = 0x02; // proposal_version low byte, little-endian
        assert_eq!(
            ProposalRecordV1::decode(&encoded),
            Err(StateError::UnsupportedProposalRecordVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_status() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        // status is the one byte immediately before the trailing eight
        // u128 tally/weight fields (8 * 16 = 128 bytes).
        let status_index = encoded.len() - (8 * 16) - 1;
        assert_eq!(encoded[status_index], ProposalStatus::Voting.as_u8());
        encoded[status_index] = 0x09;
        assert_eq!(
            ProposalRecordV1::decode(&encoded),
            Err(StateError::InvalidProposalStatus { value: 0x09 })
        );
        Ok(())
    }
}
