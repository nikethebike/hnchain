use hn_hncs::{Decoder, write_u8, write_u16};

use crate::error::{StateError, StateResult};
use crate::governance_payload::VoteChoice;

/// `vote_version` for the current `ProposalVoteRecordV1` shape
/// (ADR-0025, "Decided: State Shape").
pub const PROPOSAL_VOTE_RECORD_VERSION_1: u16 = 1;

/// Records that a voter has already cast a vote on a proposal
/// (ADR-0025, "Decided: State Shape"; "One Vote Per Sender Per
/// Proposal"). Keyed by `(proposal_id, voter)` via
/// [`crate::governance::proposal_vote_record_state_key`] — `proposal_id`
/// and `voter` are therefore not fields of this value: they are already
/// the state key, the same reasoning that keeps them out of every other
/// per-entity value in this crate that is keyed by the thing it
/// describes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProposalVoteRecordV1 {
    /// The choice this voter cast. Immutable once recorded — vote-
    /// changing is not decided (ADR-0025, Open Decisions).
    pub choice: VoteChoice,
}

impl ProposalVoteRecordV1 {
    /// Encodes this value as canonical HNCS bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3);
        write_u16(&mut out, PROPOSAL_VOTE_RECORD_VERSION_1);
        write_u8(&mut out, self.choice.as_u8());
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ProposalVoteRecordV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let vote_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if vote_version != PROPOSAL_VOTE_RECORD_VERSION_1 {
            return Err(StateError::UnsupportedProposalVoteRecordVersion {
                value: vote_version,
            });
        }

        let choice = VoteChoice::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { choice })
    }
}

#[cfg(test)]
mod tests {
    use super::{ProposalVoteRecordV1, StateError};
    use crate::error::StateResult;
    use crate::governance_payload::VoteChoice;

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        for choice in [VoteChoice::For, VoteChoice::Against, VoteChoice::Abstain] {
            let record = ProposalVoteRecordV1 { choice };
            let decoded = ProposalVoteRecordV1::decode(&record.encode())?;
            assert_eq!(decoded, record);
        }
        Ok(())
    }

    #[test]
    fn rejects_unsupported_vote_version() {
        let record = ProposalVoteRecordV1 {
            choice: VoteChoice::For,
        };
        let mut encoded = record.encode();
        encoded[0] = 0x02; // vote_version low byte, little-endian
        assert_eq!(
            ProposalVoteRecordV1::decode(&encoded),
            Err(StateError::UnsupportedProposalVoteRecordVersion { value: 2 })
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let record = ProposalVoteRecordV1 {
            choice: VoteChoice::For,
        };
        let mut encoded = record.encode();
        encoded.push(0x00);
        assert!(ProposalVoteRecordV1::decode(&encoded).is_err());
    }
}
