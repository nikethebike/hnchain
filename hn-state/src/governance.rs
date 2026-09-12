use hn_crypto::Digest;

use crate::error::StateResult;
use crate::key::state_key_core;

/// `domain_id` for the `governance` domain (ADR-0007, State Domains).
///
/// Holds both a singleton shape and a per-proposal collection, the
/// same `bridge` (`0x000A`) already established for its own registry-
/// plus-collection split — corrected from an original pure-singleton
/// mapping once a real governance model existed to test that
/// cardinality claim against (ADR-0025, "Correction," ADR-0007).
pub const DOMAIN_GOVERNANCE: u8 = 0x07;

/// The `governance` domain's SectionId registry. Deliberately not
/// closed/exhaustive the way [`crate::AccountSection`] is: only
/// `Proposal` has a decided schema today; a singleton governance
/// configuration record (ADR-0025's own Open Decisions) has no section
/// yet since it has no decided fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GovernanceSection {
    /// [`crate::ProposalRecordV1`] and [`crate::ProposalVoteRecordV1`]
    /// both live here, distinguished by `object_id`/`subkey` — not by
    /// separate sections (ADR-0025, "Decided: State Shape").
    Proposal = 0x01,
}

impl GovernanceSection {
    /// Returns the registry value for this section.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Derives the state key for a proposal's own [`crate::ProposalRecordV1`]
/// (ADR-0025, "Decided: State Shape"): `object_id = proposal_id`, empty
/// subkey.
pub fn proposal_record_state_key(proposal_id: &Digest) -> StateResult<Digest> {
    state_key_core(
        DOMAIN_GOVERNANCE,
        GovernanceSection::Proposal.as_u8(),
        proposal_id,
        &[],
    )
}

/// Derives the state key for one voter's [`crate::ProposalVoteRecordV1`]
/// on one proposal (ADR-0025, "Decided: State Shape"): `object_id =
/// proposal_id`, `subkey = voter`'s own `address_body`.
///
/// The first real use of a non-empty `subkey` anywhere in this crate —
/// every leaf class ADR-0007 itself defines uses an empty one, but
/// `subkey` exists in `state_key_core`'s own signature specifically
/// for exactly this "per-entity-within-entity" keying, and ADR-0025's
/// own "Decided: State Shape" already specifies it this way to make
/// double-voting structurally impossible (rather than a raw
/// concatenation of `proposal_id`/`voter` into one `object_id`, which
/// would need its own bespoke framing).
pub fn proposal_vote_record_state_key(proposal_id: &Digest, voter: &Digest) -> StateResult<Digest> {
    state_key_core(
        DOMAIN_GOVERNANCE,
        GovernanceSection::Proposal.as_u8(),
        proposal_id,
        voter,
    )
}

#[cfg(test)]
mod tests {
    use super::{proposal_record_state_key, proposal_vote_record_state_key};
    use crate::error::StateResult;

    const PROPOSAL_ID: [u8; 32] = [0x44; 32];
    const VOTER_A: [u8; 32] = [0x11; 32];
    const VOTER_B: [u8; 32] = [0x22; 32];

    #[test]
    fn record_and_vote_keys_are_distinct() -> StateResult<()> {
        let record_key = proposal_record_state_key(&PROPOSAL_ID)?;
        let vote_key = proposal_vote_record_state_key(&PROPOSAL_ID, &VOTER_A)?;
        assert_ne!(record_key, vote_key);
        Ok(())
    }

    #[test]
    fn different_voters_on_the_same_proposal_get_different_keys() -> StateResult<()> {
        let key_a = proposal_vote_record_state_key(&PROPOSAL_ID, &VOTER_A)?;
        let key_b = proposal_vote_record_state_key(&PROPOSAL_ID, &VOTER_B)?;
        assert_ne!(key_a, key_b);
        Ok(())
    }

    #[test]
    fn same_voter_on_different_proposals_gets_different_keys() -> StateResult<()> {
        let other_proposal = [0x99; 32];
        let key_a = proposal_vote_record_state_key(&PROPOSAL_ID, &VOTER_A)?;
        let key_b = proposal_vote_record_state_key(&other_proposal, &VOTER_A)?;
        assert_ne!(key_a, key_b);
        Ok(())
    }
}
