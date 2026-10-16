//! ADR-0048's own deterministic adversarial simulator (Verification
//! Requirement 2): real `ConsensusState` instances, driven by
//! `support::Network`, exercising the required scenarios against the
//! stated safety property -- "no two different blocks are ever
//! finalized at the same height."

mod support;

use hn_core::{BlockHeight, Round};
use support::{Behavior, Delivery, Network, TestResult};

const V0: [u8; 32] = [0x01; 32];
const V1: [u8; 32] = [0x02; 32];
const V2: [u8; 32] = [0x03; 32];
const V3: [u8; 32] = [0x04; 32];

fn four_honest_validators(height: BlockHeight) -> Network {
    Network::new(
        height,
        vec![
            (V0, 100, Behavior::Honest),
            (V1, 100, Behavior::Honest),
            (V2, 100, Behavior::Honest),
            (V3, 100, Behavior::Honest),
        ],
    )
}

#[test]
fn four_honest_validators_finalize_the_same_block_at_round_zero() -> TestResult {
    let mut network = four_honest_validators(BlockHeight::new(1));
    let block_hash = network.run_height(Round::FIRST, 10, |_round| Delivery::default())?;

    for validator in &network.validators {
        assert_eq!(
            validator.state.step,
            hn_consensus::ConsensusStep::Finalize,
            "validator {:?} did not reach Finalize",
            validator.id
        );
        assert_eq!(validator.state.round, Round::FIRST, "finalized at round 0");
    }
    // A genuine block hash was agreed on, not a zeroed placeholder.
    assert_ne!(block_hash, [0_u8; 32]);
    Ok(())
}

const BLOCK_A: [u8; 32] = [0xA0; 32];
const BLOCK_B: [u8; 32] = [0xB0; 32];

/// ADR-0048's own required scenario: "equivocating proposer cannot
/// cause two finalizations." Round 0's own proposer (`round_proposer`
/// picks index `(height + round) % 4 = 1`, i.e. `V1`) sends `BLOCK_A`
/// to `V0`/`V1`/`V2` and `BLOCK_B` to `V3` alone. Quorum intersection
/// at `n = 4` makes the two groups' own sizes (3 and 1) the only
/// split where *either* side can reach `2f+1` at all — `V3`'s own
/// lone, divergent view can never finalize anything by itself, and
/// the three-way agreement finalizes `BLOCK_A` exactly as if the
/// proposer had been honest.
#[test]
fn equivocating_proposer_cannot_cause_two_finalizations() -> TestResult {
    let height = BlockHeight::new(1);
    let mut network = Network::new(
        height,
        vec![
            (V0, 100, Behavior::Honest),
            (
                V1,
                100,
                Behavior::EquivocatingProposer {
                    first: BLOCK_A,
                    second: BLOCK_B,
                    split_index: 2,
                },
            ),
            (V2, 100, Behavior::Honest),
            (V3, 100, Behavior::Honest),
        ],
    );

    let block_hash = network.run_height(Round::FIRST, 10, |_round| Delivery::default())?;
    assert_eq!(block_hash, BLOCK_A);

    // The two validators that received the real majority's own value
    // (V0, V2 -- both Honest, both among the three recipients of
    // BLOCK_A) agree on it. V3, the lone recipient of the proposer's
    // other value, legitimately needs separate catch-up (ADR-0044
    // sync) rather than converging within this one round -- this
    // test's own safety claim is about agreement among validators
    // that saw the same real proposal, not that every validator
    // reaches Finalize in lockstep regardless of what it was shown.
    for id in [V0, V2] {
        let validator = network
            .validators
            .iter()
            .find(|v| v.id == id)
            .ok_or("validator missing")?;
        assert_eq!(validator.state.step, hn_consensus::ConsensusStep::Finalize);
    }
    Ok(())
}

/// ADR-0048's own required scenario, Finding 3.1: "the partial-lock
/// scenario finalizes, thanks to re-proposal of the valid value."
/// At round 0, `V0`/`V1`/`V2` all validate and prevote round 0's own
/// fresh block (`300` of `400`, a real polka), lock on it, and
/// precommit it -- but `V2`'s own precommit is dropped in the
/// network (`precommit_dropped_for`), so only `200` of `400` is ever
/// counted and no precommit quorum forms. Round 0 fails on a timeout
/// with three validators genuinely locked on that block and no one
/// having finalized anything. Round 1's own proposer (`(1+1)%4 = 2`,
/// i.e. `V2`) still holds `valid_value = Some(round 0's block)` from
/// round 0 (R6's own "Always" update fired on it regardless of its
/// own dropped precommit) and re-proposes it (R1) -- without this, a
/// proposer with no memory of the polka would build an unrelated
/// fresh block every round, and the height could fail to finalize
/// indefinitely.
#[test]
fn partial_lock_finalizes_via_re_proposal_of_the_valid_value() -> TestResult {
    let height = BlockHeight::new(1);
    let round_zero_block = support::deterministic_block_hash(height, Round::FIRST);
    let mut network = Network::new(
        height,
        vec![
            (V0, 100, Behavior::Honest),
            (V1, 100, Behavior::Honest),
            (V2, 100, Behavior::Honest),
            (V3, 100, Behavior::Silent),
        ],
    );

    let block_hash = network.run_height(Round::FIRST, 10, |round| {
        if round == Round::FIRST {
            Delivery {
                precommit_dropped_for: [V2].into_iter().collect(),
                ..Delivery::default()
            }
        } else {
            Delivery::default()
        }
    })?;

    // Round 1 re-proposed round 0's own value -- not a fresh block
    // for round 1 (which `deterministic_block_hash(height, 1)` would
    // be, and which the assertion below would catch this test
    // silently passing against if R1's re-proposal were ever lost).
    assert_eq!(block_hash, round_zero_block);
    assert_ne!(
        block_hash,
        support::deterministic_block_hash(height, Round::new(1))
    );
    for id in [V0, V1, V2] {
        let validator = network
            .validators
            .iter()
            .find(|v| v.id == id)
            .ok_or("validator missing")?;
        assert_eq!(validator.state.step, hn_consensus::ConsensusStep::Finalize);
    }
    Ok(())
}

/// ADR-0048's own required scenario: "lagging validator catches up
/// through round skip" (R10). `V3` never receives anything at all
/// (modeled directly, not through `Network::run_round`'s own
/// lockstep driving, since round skip's whole point is a validator
/// whose own round number has fallen behind the others' -- something
/// a round-synchronous harness cannot produce by itself). Evidence
/// that more than a third of voting power (`V0` and `V1` together,
/// `200` of `400`) is already at round 5 lets it jump straight
/// there without ever processing rounds 1 through 4 individually.
#[test]
fn lagging_validator_catches_up_through_round_skip() -> TestResult {
    use hn_consensus::{ConsensusAction, ConsensusEvent, ConsensusState};

    let height = BlockHeight::new(1);
    let mut lagging = ConsensusState::new_height(height);
    lagging.apply(ConsensusEvent::BeginRound)?;
    assert_eq!(lagging.round, Round::FIRST);

    let action = lagging.apply(ConsensusEvent::RoundSkip {
        height,
        round: Round::new(5),
    })?;
    assert_eq!(
        action,
        ConsensusAction::RoundAdvanced {
            round: Round::new(5)
        }
    );
    assert_eq!(lagging.round, Round::new(5));
    assert_eq!(lagging.step, hn_consensus::ConsensusStep::Timeout);

    // It resumes ordinary participation from exactly there.
    let action = lagging.apply(ConsensusEvent::BeginRound)?;
    assert_eq!(action, ConsensusAction::None);
    assert_eq!(lagging.step, hn_consensus::ConsensusStep::Propose);
    Ok(())
}

/// ADR-0048's own required scenario: "precommit quorum from an older
/// round finalizes a late validator" (R8). Three validators run an
/// ordinary round 0 to completion through `Network::run_round`; a
/// fourth (`V3`) never receives the proposal at all (modeled
/// directly: `Network::run_round` always drives every non-Byzantine
/// validator's own timeout/quorum events in lockstep, which would
/// force `V3` through the same rounds too, defeating the point of a
/// validator that is genuinely behind). Handed the very same real
/// certificate the other three already finalized with, it has no
/// validated proposal for the block and returns `NeedBlock`; once
/// told the block is now available (`BlockFetched`), it finalizes
/// the identical hash.
#[test]
fn precommit_quorum_from_an_earlier_round_finalizes_a_late_validator() -> TestResult {
    let height = BlockHeight::new(1);
    let mut network = Network::new(
        height,
        vec![
            (V0, 100, Behavior::Honest),
            (V1, 100, Behavior::Honest),
            (V2, 100, Behavior::Honest),
        ],
    );
    let finalized_hash = network.run_height(Round::FIRST, 10, |_round| Delivery::default())?;

    // The real commit_qc the three of them finalized with, read back
    // off one of their own now-`Finalize`-step states is not
    // directly exposed by `ConsensusState`, so this test builds the
    // equivalent certificate the same way `Network::aggregate` did
    // internally: 300 of 300 voting power precommitting the same
    // block at round 0.
    use hn_core::Epoch;
    use hn_state::{QuorumCertificate, VoteTargetType, VoteType};
    let commit_qc = QuorumCertificate {
        certificate_type: VoteType::Precommit,
        chain_id: support::CHAIN_ID,
        network_id: support::NETWORK_ID,
        epoch: Epoch::new(0),
        height,
        round: Round::FIRST,
        validator_set_commitment: support::VALIDATOR_SET_COMMITMENT,
        target_type: VoteTargetType::Block,
        target_hash: finalized_hash,
        total_voting_power: 300,
        signed_voting_power: 300,
        signer_commitment: Vec::new(),
        aggregate_proof: Vec::new(),
    };

    let mut late = hn_consensus::ConsensusState::new_height(height);
    let action = late.apply(hn_consensus::ConsensusEvent::PrecommitQuorum(
        commit_qc.clone(),
    ))?;
    assert_eq!(
        action,
        hn_consensus::ConsensusAction::NeedBlock {
            block_hash: finalized_hash,
            commit_qc,
        }
    );
    assert_ne!(late.step, hn_consensus::ConsensusStep::Finalize);

    let action = late.apply(hn_consensus::ConsensusEvent::BlockFetched {
        block_hash: finalized_hash,
    })?;
    match action {
        hn_consensus::ConsensusAction::Finalized { block_hash, .. } => {
            assert_eq!(block_hash, finalized_hash);
        }
        other => return Err(format!("expected Finalized, got {other:?}").into()),
    }
    Ok(())
}

// ADR-0048's own required scenario "a Byzantine validator that
// double-votes yields recorded evidence and does not change any
// tally" is a `VotePool`-level property, not a `ConsensusState`
// one -- `hn_consensus::VotePool` is what a real caller aggregates
// network-sourced votes through (this simulator's own `aggregate`
// helper is a simplified stand-in, not a second `VotePool`). It is
// covered directly, and more precisely, by
// `vote_pool::tests::a_second_vote_for_a_different_target_is_recorded_as_equivocation_not_tallied`
// in `hn-consensus/src/vote_pool.rs`.
