//! ADR-0048's own "Verification Requirements" item 3: exhaustive
//! bounded state exploration, run in CI (n = 4, f = 1, bounded
//! rounds), asserting the safety property after every single run
//! rather than sampling it.
//!
//! **Scope, stated explicitly (the result is "safe within these
//! bounds", never "safe"):** for n = 4 and a 3-round bound, this
//! enumerates every choice of which single validator (if any) is
//! Byzantine, crossed with a fixed menu of that validator's own
//! behavior, crossed with every possible per-round subset of
//! validators whose proposal is dropped. It does *not* also vary
//! `precommit_dropped_for` (a second full `2^n` per round) or
//! `EquivocatingProposer`'s own split point -- either would multiply
//! the explored space further for a CI run whose whole point is
//! staying under a few minutes. `byzantine_simulator.rs`'s own named
//! scenarios already exercise `precommit_dropped_for` and a specific
//! split by hand; the random-schedule property test (a later commit
//! in this phase) covers a wider space, including `n = 7`, where
//! full enumeration is too big.

mod support;

use std::collections::BTreeSet;

use hn_consensus::ConsensusTarget;
use hn_core::{BlockHeight, Round};
use support::{Behavior, Delivery, Digest, Network, TestResult};

const ROUND_BOUND: u64 = 3;
const FIXED_ADVERSARIAL_BLOCK: Digest = [0xDE; 32];

fn behavior_menu() -> Vec<Behavior> {
    vec![
        Behavior::Silent,
        Behavior::VoteForAnything(ConsensusTarget::Nil),
        Behavior::VoteForAnything(ConsensusTarget::Block(FIXED_ADVERSARIAL_BLOCK)),
        Behavior::EquivocatingProposer {
            first: FIXED_ADVERSARIAL_BLOCK,
            second: [0xEF; 32],
            split_index: 1,
        },
    ]
}

/// Runs one fully-specified configuration (a fixed Byzantine choice
/// and behavior, if any) against every possible per-round proposal-
/// drop subset over `ROUND_BOUND` rounds, for `ids`. Returns the
/// number of distinct schedules actually run. A height that never
/// finalizes within the round bound is accepted (the bound is a test
/// budget, not a liveness claim this explorer is checking) -- only
/// `Network::run_round`'s own internal "two different blocks
/// finalized" check, surfacing as any *other* error, fails the test.
fn explore_one_configuration(
    ids: &[Digest],
    byzantine_index: Option<usize>,
    byzantine_behavior: Option<&Behavior>,
) -> TestResult<u64> {
    let n = ids.len();
    let subsets_per_round: u64 = 1 << n;
    let total_schedules = subsets_per_round.pow(u32::try_from(ROUND_BOUND)?);

    let members: Vec<(Digest, u128, Behavior)> = ids
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            let behavior = if Some(index) == byzantine_index {
                byzantine_behavior.cloned().unwrap_or(Behavior::Honest)
            } else {
                Behavior::Honest
            };
            (id, 100, behavior)
        })
        .collect();

    for schedule in 0..total_schedules {
        let height = BlockHeight::new(1);
        let mut network = Network::new(height, members.clone());
        let mut remaining = schedule;
        let outcome = network.run_height(Round::FIRST, ROUND_BOUND, |_round| {
            let subset_bits = remaining % subsets_per_round;
            remaining /= subsets_per_round;
            let dropped: BTreeSet<Digest> = ids
                .iter()
                .enumerate()
                .filter(|(index, _)| subset_bits & (1 << index) != 0)
                .map(|(_, id)| *id)
                .collect();
            Delivery {
                proposal_dropped_for: dropped,
                precommit_dropped_for: BTreeSet::new(),
            }
        });

        match outcome {
            Ok(_) => {}
            Err(error) if error.to_string().contains("did not finalize within") => {}
            Err(error) => {
                return Err(format!(
                    "configuration (byzantine_index={byzantine_index:?}, \
                     behavior={byzantine_behavior:?}) schedule {schedule} of \
                     {total_schedules}: {error}"
                )
                .into());
            }
        }
    }
    Ok(total_schedules)
}

#[test]
fn exhaustive_exploration_n4_f1_three_rounds() -> TestResult {
    let ids: Vec<Digest> = vec![[0x01; 32], [0x02; 32], [0x03; 32], [0x04; 32]];
    let menu = behavior_menu();
    let mut total_runs: u64 = 0;

    // No Byzantine validator at all.
    total_runs += explore_one_configuration(&ids, None, None)?;

    // Exactly one of the four is Byzantine, for every behavior in
    // the menu -- n = 4, f = 1 means this is the entire Byzantine
    // budget; two simultaneously-Byzantine validators would already
    // be outside the stated adversary assumption.
    for byzantine_index in 0..ids.len() {
        for behavior in &menu {
            total_runs += explore_one_configuration(&ids, Some(byzantine_index), Some(behavior))?;
        }
    }

    assert!(
        total_runs > 60_000,
        "expected tens of thousands of schedules actually explored, got {total_runs}"
    );
    Ok(())
}
