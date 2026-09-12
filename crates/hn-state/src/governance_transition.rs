use hn_core::BlockHeight;
use hn_crypto::Digest;

use crate::{
    error::{StateError, StateResult},
    governance::{proposal_record_state_key, proposal_vote_record_state_key},
    governance_payload::{GovernancePayloadV1, VoteChoice},
    node::{leaf_hash, value_hash},
    proposal_id::proposal_id,
    proposal_record::{ProposalRecordV1, ProposalStatus},
    proposal_vote_record::ProposalVoteRecordV1,
    receipt::{ReceiptStatus, ReceiptV1},
    tree::Leaf,
    validator_record::{ValidatorRecordV1, ValidatorStatus},
};

/// [`apply_propose`]'s own success value: the new proposal's id, plus
/// the one write-set leaf its initial record occupies.
pub type ProposeOutcome = (Digest, [Leaf; 1]);

/// Creates a new proposal (ADR-0025, "Decided: State Shape"),
/// producing its own `proposal_id` and the one write-set leaf its
/// initial `ProposalRecordV1` occupies.
///
/// `proposer_record` is `sender`'s own `ValidatorRecordV1` — mirroring
/// `apply_stake`/`apply_unstake`'s own "caller supplies the sender's
/// already-fetched record" pattern, this function does not look
/// anything up itself. Checks the validation precondition that
/// `proposer_record.status == Active` ([`StateError::ProposerMustBeActive`],
/// ADR-0025 "Decided: Who May Propose").
///
/// `validator_chamber_total_weight`/`staker_chamber_total_weight` are
/// the caller-computed chamber snapshots taken at `created_at_height`
/// (ADR-0025, "Snapshot Determinism") — this crate owns state
/// *transitions*, not the query that produces a total validator count
/// or a total bonded-stake sum, the same boundary [`crate::active_set`]
/// already draws for its own candidate slice.
///
/// `voting_window_blocks` is `GOVERNANCE_VOTING_WINDOW` (ADR-0023, still
/// an open economic parameter) — a parameter here, not a constant, so
/// this function does not have to change once that value is decided,
/// the same reasoning [`crate::active_set`]'s own `max_size` parameter
/// already used for `MAX_ACTIVE_SET_SIZE` before it was decided.
pub fn apply_propose(
    sender: Digest,
    proposer_record: &ValidatorRecordV1,
    payload: &GovernancePayloadV1,
    created_at_height: BlockHeight,
    voting_window_blocks: u64,
    validator_chamber_total_weight: u128,
    staker_chamber_total_weight: u128,
) -> StateResult<ProposeOutcome> {
    let GovernancePayloadV1::Propose {
        title,
        content_hash,
    } = payload
    else {
        return Err(StateError::ExpectedProposeOperation);
    };

    if proposer_record.status != ValidatorStatus::Active {
        return Err(StateError::ProposerMustBeActive);
    }

    let proposal_id_value = proposal_id(payload)?;
    let voting_ends_at_height = created_at_height
        .get()
        .checked_add(voting_window_blocks)
        .ok_or(StateError::VotingWindowHeightOverflow)?;

    let record = ProposalRecordV1 {
        proposal_id: proposal_id_value,
        proposer: sender,
        title: title.clone(),
        content_hash: *content_hash,
        created_at_height,
        voting_ends_at_height: BlockHeight::new(voting_ends_at_height),
        status: ProposalStatus::Voting,
        validator_chamber_for: 0,
        validator_chamber_against: 0,
        validator_chamber_abstain: 0,
        staker_chamber_for: 0,
        staker_chamber_against: 0,
        staker_chamber_abstain: 0,
        validator_chamber_total_weight,
        staker_chamber_total_weight,
    };

    Ok((proposal_id_value, [proposal_record_leaf(&record)?]))
}

/// Applies a `propose` and produces its [`ReceiptV1`] in one step,
/// mirroring [`crate::apply_stake_with_receipt`]'s own shape.
/// [`StateError::ProposerMustBeActive`]/
/// [`StateError::VotingWindowHeightOverflow`] are legitimate transaction
/// outcomes; anything else (including
/// [`StateError::ExpectedProposeOperation`], which indicates a dispatch
/// bug — this function should only ever be called for a `Propose`
/// payload) is a real internal error and propagates.
#[allow(clippy::too_many_arguments)]
pub fn apply_propose_with_receipt(
    sender: Digest,
    proposer_record: &ValidatorRecordV1,
    payload: &GovernancePayloadV1,
    created_at_height: BlockHeight,
    voting_window_blocks: u64,
    validator_chamber_total_weight: u128,
    staker_chamber_total_weight: u128,
    tx_id: Digest,
) -> StateResult<(Option<ProposeOutcome>, ReceiptV1)> {
    match apply_propose(
        sender,
        proposer_record,
        payload,
        created_at_height,
        voting_window_blocks,
        validator_chamber_total_weight,
        staker_chamber_total_weight,
    ) {
        Ok(result) => Ok((
            Some(result),
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Success,
            },
        )),
        Err(StateError::ProposerMustBeActive | StateError::VotingWindowHeightOverflow) => Ok((
            None,
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Failed,
            },
        )),
        Err(other) => Err(other),
    }
}

/// Casts a vote on an existing proposal (ADR-0025, "Decided: Chambers,
/// Membership, And Weight" / "Decided: Chamber Pass Rule"), producing
/// the updated `ProposalRecordV1` leaf and a new `ProposalVoteRecordV1`
/// leaf that makes a second vote from the same sender structurally
/// rejected ("One Vote Per Sender Per Proposal").
///
/// `voter_record` is `sender`'s own `ValidatorRecordV1`, if any —
/// `None` for a sender with no validator record at all. Weight in each
/// chamber is derived from it: one vote in the validator chamber iff
/// `status == Active`; `bonded_stake` weight in the staker chamber
/// regardless of status (a validator is also a staker of their own
/// bonded funds — see ADR-0025's own "Decided: Chambers" for why this
/// is not a double-count). A sender contributing zero weight to both
/// chambers is rejected
/// ([`StateError::NoGovernanceVotingWeight`]) rather than recorded as a
/// no-op vote.
///
/// `proposal` is the target proposal, already fetched by the caller
/// (this crate's own state-interfaces-not-storage boundary, as
/// everywhere else). If the `Vote` payload's own `proposal_id` does not
/// match `proposal.proposal_id`, that is treated as
/// [`StateError::UnknownProposal`] — the caller fetched (or was asked
/// to operate on) the wrong record. `existing_vote` is the sender's own
/// prior [`ProposalVoteRecordV1`] for this proposal, if any —
/// `Some(_)` is rejected as [`StateError::ProposalAlreadyVoted`].
pub fn apply_vote(
    sender: Digest,
    voter_record: Option<&ValidatorRecordV1>,
    payload: &GovernancePayloadV1,
    proposal: &ProposalRecordV1,
    existing_vote: Option<&ProposalVoteRecordV1>,
    current_height: BlockHeight,
) -> StateResult<[Leaf; 2]> {
    let GovernancePayloadV1::Vote {
        proposal_id: voted_proposal_id,
        choice,
    } = payload
    else {
        return Err(StateError::ExpectedVoteOperation);
    };

    if *voted_proposal_id != proposal.proposal_id {
        return Err(StateError::UnknownProposal);
    }

    if proposal.status != ProposalStatus::Voting
        || current_height.get() >= proposal.voting_ends_at_height.get()
    {
        return Err(StateError::VotingWindowClosed);
    }

    if existing_vote.is_some() {
        return Err(StateError::ProposalAlreadyVoted);
    }

    let validator_weight =
        u128::from(voter_record.is_some_and(|record| record.status == ValidatorStatus::Active));
    let staker_weight = voter_record.map_or(0, |record| record.bonded_stake);

    if validator_weight == 0 && staker_weight == 0 {
        return Err(StateError::NoGovernanceVotingWeight);
    }

    let mut updated = proposal.clone();
    let (validator_tally, staker_tally) = match choice {
        VoteChoice::For => (
            &mut updated.validator_chamber_for,
            &mut updated.staker_chamber_for,
        ),
        VoteChoice::Against => (
            &mut updated.validator_chamber_against,
            &mut updated.staker_chamber_against,
        ),
        VoteChoice::Abstain => (
            &mut updated.validator_chamber_abstain,
            &mut updated.staker_chamber_abstain,
        ),
    };
    *validator_tally = validator_tally
        .checked_add(validator_weight)
        .ok_or(StateError::GovernanceTallyOverflow)?;
    *staker_tally = staker_tally
        .checked_add(staker_weight)
        .ok_or(StateError::GovernanceTallyOverflow)?;

    let vote_record = ProposalVoteRecordV1 { choice: *choice };

    Ok([
        proposal_record_leaf(&updated)?,
        proposal_vote_record_leaf(&proposal.proposal_id, &sender, &vote_record)?,
    ])
}

/// Applies a `vote` and produces its [`ReceiptV1`] in one step. Every
/// one of [`apply_vote`]'s own validation-precondition failures
/// ([`StateError::UnknownProposal`], [`StateError::VotingWindowClosed`],
/// [`StateError::ProposalAlreadyVoted`],
/// [`StateError::NoGovernanceVotingWeight`],
/// [`StateError::GovernanceTallyOverflow`]) is a legitimate transaction
/// outcome; [`StateError::ExpectedVoteOperation`] indicates a dispatch
/// bug and propagates instead.
pub fn apply_vote_with_receipt(
    sender: Digest,
    voter_record: Option<&ValidatorRecordV1>,
    payload: &GovernancePayloadV1,
    proposal: &ProposalRecordV1,
    existing_vote: Option<&ProposalVoteRecordV1>,
    current_height: BlockHeight,
    tx_id: Digest,
) -> StateResult<(Option<[Leaf; 2]>, ReceiptV1)> {
    match apply_vote(
        sender,
        voter_record,
        payload,
        proposal,
        existing_vote,
        current_height,
    ) {
        Ok(leaves) => Ok((
            Some(leaves),
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Success,
            },
        )),
        Err(
            StateError::UnknownProposal
            | StateError::VotingWindowClosed
            | StateError::ProposalAlreadyVoted
            | StateError::NoGovernanceVotingWeight
            | StateError::GovernanceTallyOverflow,
        ) => Ok((
            None,
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Failed,
            },
        )),
        Err(other) => Err(other),
    }
}

/// Closes a proposal once its voting window has passed, computing its
/// final outcome (ADR-0025, "Decided: Proposal Outcome States").
///
/// Returns `Ok(None)`, not an error, whenever there is nothing to do:
/// the proposal is not currently `Voting`, or its voting window has not
/// closed yet at `current_height`. Mirrors
/// [`crate::apply_unbonding_release`]'s own "periodic sweep, no caller
/// yet" situation — the intended caller is a per-block pass over every
/// still-`Voting` proposal, which does not exist yet (no
/// block-processing pipeline exists anywhere in this codebase,
/// `hn-consensus`/`hn-node` are still stubs).
///
/// `quorum_numerator`/`quorum_denominator` are `GOVERNANCE_QUORUM`
/// (ADR-0023, still an open economic parameter) — a parameter here for
/// the same reason `voting_window_blocks` is one in [`apply_propose`].
/// Quorum is checked via cross-multiplication
/// (`participated * quorum_denominator >= total_weight *
/// quorum_numerator`), never floating-point division (ADR-0000).
pub fn finalize_proposal(
    proposal: &ProposalRecordV1,
    current_height: BlockHeight,
    quorum_numerator: u128,
    quorum_denominator: u128,
) -> StateResult<Option<[Leaf; 1]>> {
    if proposal.status != ProposalStatus::Voting {
        return Ok(None);
    }
    if current_height.get() < proposal.voting_ends_at_height.get() {
        return Ok(None);
    }

    let validator_participated = sum3(
        proposal.validator_chamber_for,
        proposal.validator_chamber_against,
        proposal.validator_chamber_abstain,
    )?;
    let staker_participated = sum3(
        proposal.staker_chamber_for,
        proposal.staker_chamber_against,
        proposal.staker_chamber_abstain,
    )?;

    let validator_quorum_met = meets_quorum(
        validator_participated,
        proposal.validator_chamber_total_weight,
        quorum_numerator,
        quorum_denominator,
    )?;
    let staker_quorum_met = meets_quorum(
        staker_participated,
        proposal.staker_chamber_total_weight,
        quorum_numerator,
        quorum_denominator,
    )?;

    let status = if validator_quorum_met && staker_quorum_met {
        let validator_passed = proposal.validator_chamber_for > proposal.validator_chamber_against;
        let staker_passed = proposal.staker_chamber_for > proposal.staker_chamber_against;
        if validator_passed && staker_passed {
            ProposalStatus::Passed
        } else {
            ProposalStatus::Rejected
        }
    } else {
        ProposalStatus::Expired
    };

    let updated = ProposalRecordV1 {
        status,
        ..proposal.clone()
    };
    Ok(Some([proposal_record_leaf(&updated)?]))
}

fn sum3(a: u128, b: u128, c: u128) -> StateResult<u128> {
    a.checked_add(b)
        .and_then(|sum| sum.checked_add(c))
        .ok_or(StateError::GovernanceTallyOverflow)
}

fn meets_quorum(
    participated: u128,
    total_weight: u128,
    quorum_numerator: u128,
    quorum_denominator: u128,
) -> StateResult<bool> {
    let lhs = participated
        .checked_mul(quorum_denominator)
        .ok_or(StateError::GovernanceTallyOverflow)?;
    let rhs = total_weight
        .checked_mul(quorum_numerator)
        .ok_or(StateError::GovernanceTallyOverflow)?;
    Ok(lhs >= rhs)
}

fn proposal_record_leaf(record: &ProposalRecordV1) -> StateResult<Leaf> {
    let key = proposal_record_state_key(&record.proposal_id)?;
    let value_bytes = record.encode()?;
    let vh = value_hash(&value_bytes)?;
    Ok((key, leaf_hash(&key, &vh)?))
}

fn proposal_vote_record_leaf(
    proposal_id: &Digest,
    voter: &Digest,
    record: &ProposalVoteRecordV1,
) -> StateResult<Leaf> {
    let key = proposal_vote_record_state_key(proposal_id, voter)?;
    let value_bytes = record.encode();
    let vh = value_hash(&value_bytes)?;
    Ok((key, leaf_hash(&key, &vh)?))
}

#[cfg(test)]
mod tests {
    use hn_core::BlockHeight;
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::{
        apply_propose, apply_propose_with_receipt, apply_vote, apply_vote_with_receipt,
        finalize_proposal,
    };
    use crate::{
        error::{StateError, StateResult},
        governance_payload::{GovernancePayloadV1, VoteChoice},
        proposal_record::{ProposalRecordV1, ProposalStatus},
        proposal_vote_record::ProposalVoteRecordV1,
        receipt::ReceiptStatus,
        validator_record::{ValidatorRecordV1, ValidatorStatus},
    };

    const SENDER: [u8; 32] = [0x11; 32];
    const TX_ID: [u8; 32] = [0x99; 32];

    fn validator(status: ValidatorStatus, bonded_stake: u128) -> ValidatorRecordV1 {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x01; 32]);
        ValidatorRecordV1 {
            validator_id: SENDER,
            consensus_key: keypair.key_descriptor(),
            bonded_stake,
            voting_power: bonded_stake,
            status,
            pending_unbonding: None,
        }
    }

    fn propose_payload() -> GovernancePayloadV1 {
        GovernancePayloadV1::Propose {
            title: "Raise MAX_ACTIVE_SET_SIZE".to_string(),
            content_hash: [0xaa; 32],
        }
    }

    #[test]
    fn propose_succeeds_from_an_active_validator() -> StateResult<()> {
        let proposer = validator(ValidatorStatus::Active, 1_000);
        let payload = propose_payload();
        let (proposal_id, [leaf]) = apply_propose(
            SENDER,
            &proposer,
            &payload,
            BlockHeight::new(100),
            500,
            20,
            1_000_000,
        )?;

        let expected_record = ProposalRecordV1 {
            proposal_id,
            proposer: SENDER,
            title: "Raise MAX_ACTIVE_SET_SIZE".to_string(),
            content_hash: [0xaa; 32],
            created_at_height: BlockHeight::new(100),
            voting_ends_at_height: BlockHeight::new(600),
            status: ProposalStatus::Voting,
            validator_chamber_for: 0,
            validator_chamber_against: 0,
            validator_chamber_abstain: 0,
            staker_chamber_for: 0,
            staker_chamber_against: 0,
            staker_chamber_abstain: 0,
            validator_chamber_total_weight: 20,
            staker_chamber_total_weight: 1_000_000,
        };
        assert_eq!(leaf.1, super::proposal_record_leaf(&expected_record)?.1);
        Ok(())
    }

    #[test]
    fn propose_rejects_a_non_active_proposer() {
        let proposer = validator(ValidatorStatus::Candidate, 1_000);
        let payload = propose_payload();
        assert_eq!(
            apply_propose(
                SENDER,
                &proposer,
                &payload,
                BlockHeight::new(100),
                500,
                20,
                1_000_000
            ),
            Err(StateError::ProposerMustBeActive)
        );
    }

    #[test]
    fn propose_with_receipt_yields_failed_for_a_non_active_proposer() -> StateResult<()> {
        let proposer = validator(ValidatorStatus::Inactive, 1_000);
        let payload = propose_payload();
        let (result, receipt) = apply_propose_with_receipt(
            SENDER,
            &proposer,
            &payload,
            BlockHeight::new(100),
            500,
            20,
            1_000_000,
            TX_ID,
        )?;
        assert!(result.is_none());
        assert_eq!(receipt.status, ReceiptStatus::Failed);
        Ok(())
    }

    fn sample_proposal() -> ProposalRecordV1 {
        ProposalRecordV1 {
            proposal_id: [0x44; 32],
            proposer: SENDER,
            title: "Raise MAX_ACTIVE_SET_SIZE".to_string(),
            content_hash: [0xaa; 32],
            created_at_height: BlockHeight::new(100),
            voting_ends_at_height: BlockHeight::new(600),
            status: ProposalStatus::Voting,
            validator_chamber_for: 0,
            validator_chamber_against: 0,
            validator_chamber_abstain: 0,
            staker_chamber_for: 0,
            staker_chamber_against: 0,
            staker_chamber_abstain: 0,
            validator_chamber_total_weight: 20,
            staker_chamber_total_weight: 1_000_000,
        }
    }

    fn vote_payload(choice: VoteChoice) -> GovernancePayloadV1 {
        GovernancePayloadV1::Vote {
            proposal_id: [0x44; 32],
            choice,
        }
    }

    #[test]
    fn vote_adds_weight_to_both_chambers_for_an_active_validator() -> StateResult<()> {
        let voter = validator(ValidatorStatus::Active, 500);
        let proposal = sample_proposal();
        let payload = vote_payload(VoteChoice::For);

        let [record_leaf, _vote_leaf] = apply_vote(
            SENDER,
            Some(&voter),
            &payload,
            &proposal,
            None,
            BlockHeight::new(200),
        )?;

        let expected = ProposalRecordV1 {
            validator_chamber_for: 1,
            staker_chamber_for: 500,
            ..proposal
        };
        assert_eq!(record_leaf.1, super::proposal_record_leaf(&expected)?.1);
        Ok(())
    }

    #[test]
    fn vote_adds_only_staker_weight_for_an_inactive_validator() -> StateResult<()> {
        let voter = validator(ValidatorStatus::Inactive, 500);
        let proposal = sample_proposal();
        let payload = vote_payload(VoteChoice::Against);

        let [record_leaf, _vote_leaf] = apply_vote(
            SENDER,
            Some(&voter),
            &payload,
            &proposal,
            None,
            BlockHeight::new(200),
        )?;

        let expected = ProposalRecordV1 {
            validator_chamber_against: 0,
            staker_chamber_against: 500,
            ..proposal
        };
        assert_eq!(record_leaf.1, super::proposal_record_leaf(&expected)?.1);
        Ok(())
    }

    #[test]
    fn vote_rejects_a_sender_with_no_governance_weight() {
        let proposal = sample_proposal();
        let payload = vote_payload(VoteChoice::Abstain);
        assert_eq!(
            apply_vote(
                SENDER,
                None,
                &payload,
                &proposal,
                None,
                BlockHeight::new(200)
            ),
            Err(StateError::NoGovernanceVotingWeight)
        );
    }

    #[test]
    fn vote_rejects_a_mismatched_proposal_id() {
        let voter = validator(ValidatorStatus::Active, 500);
        let proposal = sample_proposal();
        let payload = GovernancePayloadV1::Vote {
            proposal_id: [0xff; 32],
            choice: VoteChoice::For,
        };
        assert_eq!(
            apply_vote(
                SENDER,
                Some(&voter),
                &payload,
                &proposal,
                None,
                BlockHeight::new(200)
            ),
            Err(StateError::UnknownProposal)
        );
    }

    #[test]
    fn vote_rejects_after_the_voting_window_closes() {
        let voter = validator(ValidatorStatus::Active, 500);
        let proposal = sample_proposal();
        let payload = vote_payload(VoteChoice::For);
        assert_eq!(
            apply_vote(
                SENDER,
                Some(&voter),
                &payload,
                &proposal,
                None,
                BlockHeight::new(600)
            ),
            Err(StateError::VotingWindowClosed)
        );
    }

    #[test]
    fn vote_rejects_a_second_vote_from_the_same_sender() {
        let voter = validator(ValidatorStatus::Active, 500);
        let proposal = sample_proposal();
        let payload = vote_payload(VoteChoice::For);
        let existing = ProposalVoteRecordV1 {
            choice: VoteChoice::Against,
        };
        assert_eq!(
            apply_vote(
                SENDER,
                Some(&voter),
                &payload,
                &proposal,
                Some(&existing),
                BlockHeight::new(200)
            ),
            Err(StateError::ProposalAlreadyVoted)
        );
    }

    #[test]
    fn vote_with_receipt_yields_failed_for_a_closed_window() -> StateResult<()> {
        let voter = validator(ValidatorStatus::Active, 500);
        let proposal = sample_proposal();
        let payload = vote_payload(VoteChoice::For);
        let (result, receipt) = apply_vote_with_receipt(
            SENDER,
            Some(&voter),
            &payload,
            &proposal,
            None,
            BlockHeight::new(600),
            TX_ID,
        )?;
        assert!(result.is_none());
        assert_eq!(receipt.status, ReceiptStatus::Failed);
        Ok(())
    }

    #[test]
    fn finalize_is_a_no_op_before_the_window_closes() -> StateResult<()> {
        let proposal = sample_proposal();
        assert_eq!(
            finalize_proposal(&proposal, BlockHeight::new(599), 2, 3)?,
            None
        );
        Ok(())
    }

    #[test]
    fn finalize_expires_a_proposal_that_missed_quorum() -> StateResult<()> {
        let mut proposal = sample_proposal();
        // Only 1 of 20 validator-chamber weight participated: well
        // under a 2/3 quorum.
        proposal.validator_chamber_for = 1;
        proposal.staker_chamber_for = 900_000; // staker chamber meets quorum on its own

        let [leaf] = finalize_proposal(&proposal, BlockHeight::new(600), 2, 3)?
            .ok_or(StateError::UnknownProposal)?;
        let expected = ProposalRecordV1 {
            status: ProposalStatus::Expired,
            ..proposal
        };
        assert_eq!(leaf.1, super::proposal_record_leaf(&expected)?.1);
        Ok(())
    }

    #[test]
    fn finalize_rejects_a_proposal_that_meets_quorum_but_loses_one_chamber() -> StateResult<()> {
        let mut proposal = sample_proposal();
        proposal.validator_chamber_for = 20; // full validator turnout, unanimous for
        proposal.staker_chamber_against = 900_000; // staker chamber meets quorum, votes against

        let [leaf] = finalize_proposal(&proposal, BlockHeight::new(600), 2, 3)?
            .ok_or(StateError::UnknownProposal)?;
        let expected = ProposalRecordV1 {
            status: ProposalStatus::Rejected,
            ..proposal
        };
        assert_eq!(leaf.1, super::proposal_record_leaf(&expected)?.1);
        Ok(())
    }

    #[test]
    fn finalize_passes_a_proposal_that_meets_quorum_and_majority_in_both_chambers()
    -> StateResult<()> {
        let mut proposal = sample_proposal();
        proposal.validator_chamber_for = 15;
        proposal.validator_chamber_against = 5;
        proposal.staker_chamber_for = 800_000;
        proposal.staker_chamber_against = 100_000;

        let [leaf] = finalize_proposal(&proposal, BlockHeight::new(600), 2, 3)?
            .ok_or(StateError::UnknownProposal)?;
        let expected = ProposalRecordV1 {
            status: ProposalStatus::Passed,
            ..proposal
        };
        assert_eq!(leaf.1, super::proposal_record_leaf(&expected)?.1);
        Ok(())
    }
}
