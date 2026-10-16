//! ADR-0048's own "Verification Requirements" item 3's second half:
//! random schedules over the simulator, with up to `f` Byzantine
//! validators acting randomly, asserting "no two different blocks
//! finalized at the same height" after every run -- covering `n = 7`
//! and longer horizons where the exhaustive explorer
//! (`exhaustive_exploration.rs`) would be too big to run in CI.
//!
//! Seeds are fixed (`0..SEED_COUNT`, deterministic xorshift64* --
//! this workspace's own established dependency-free PRNG convention,
//! e.g. `hn-rpc/src/parser.rs`'s `never_panics_on_arbitrary_byte_input`),
//! so a failure here is always reproducible from the seed alone. A
//! seed that finds a real safety violation should be promoted to a
//! named regression test (ADR-0048's own Verification Requirements:
//! "Failing seeds are saved as named regression tests") -- none has
//! been found by this run, so there is nothing to promote yet.

mod support;

use std::collections::BTreeSet;

use hn_consensus::ConsensusTarget;
use hn_core::{BlockHeight, Round};
use support::{Behavior, Delivery, Digest, Network, TestResult};

const SEED_COUNT_N4: u64 = 2_000;
const SEED_COUNT_N7: u64 = 2_000;
const ROUNDS_N4: u64 = 20;
const ROUNDS_N7: u64 = 30;

struct Xorshift64(u64);

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        // 0 is a fixed point of xorshift; nudge it off.
        Self(seed ^ 0x2545_f491_4f6c_dd1d)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

fn validator_ids(n: usize) -> Vec<Digest> {
    (0..n)
        .map(|index| {
            let mut id = [0_u8; 32];
            id[0] = u8::try_from(index + 1).unwrap_or(0xFF);
            id
        })
        .collect()
}

fn random_behavior(rng: &mut Xorshift64, adversarial_block: Digest) -> Behavior {
    match rng.next_below(4) {
        0 => Behavior::Silent,
        1 => Behavior::VoteForAnything(ConsensusTarget::Nil),
        2 => Behavior::VoteForAnything(ConsensusTarget::Block(adversarial_block)),
        _ => Behavior::EquivocatingProposer {
            first: adversarial_block,
            second: {
                let mut other = adversarial_block;
                other[1] ^= 0xFF;
                other
            },
            split_index: usize::try_from(rng.next_below(4)).unwrap_or(0),
        },
    }
}

/// Runs one random schedule for `seed`: up to `f` of `n` validators
/// are Byzantine (a random count from `0` to `f` inclusive, random
/// distinct indices, each with an independently random behavior from
/// `random_behavior`'s own menu), and every round's own per-validator
/// proposal delivery is independently dropped with roughly 1-in-4
/// odds -- real asynchrony, not just the exhaustive explorer's own
/// enumerated subsets. Returns `Ok` if the height finalized safely or
/// legitimately ran out of rounds; `Err` only for a real safety
/// violation (`Network::run_round`'s own internal check) or an
/// unexpected internal error.
fn run_random_schedule(seed: u64, n: usize, f: usize, rounds: u64) -> TestResult {
    let mut rng = Xorshift64::new(seed);
    let ids = validator_ids(n);
    let adversarial_block = {
        let mut block = [0_u8; 32];
        block[31] = 0xAD;
        block
    };

    let byzantine_count = usize::try_from(rng.next_below(u64::try_from(f)? + 1))?;
    let mut remaining_indices: Vec<usize> = (0..n).collect();
    let mut byzantine_indices: BTreeSet<usize> = BTreeSet::new();
    for _ in 0..byzantine_count {
        if remaining_indices.is_empty() {
            break;
        }
        let pick = usize::try_from(rng.next_below(u64::try_from(remaining_indices.len())?))?;
        byzantine_indices.insert(remaining_indices.remove(pick));
    }

    let members: Vec<(Digest, u128, Behavior)> = ids
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            let behavior = if byzantine_indices.contains(&index) {
                random_behavior(&mut rng, adversarial_block)
            } else {
                Behavior::Honest
            };
            (id, 100, behavior)
        })
        .collect();

    let height = BlockHeight::new(1);
    let mut network = Network::new(height, members);
    let outcome = network.run_height(Round::FIRST, rounds, |_round| {
        let dropped: BTreeSet<Digest> = ids
            .iter()
            .filter(|_| rng.next_below(4) == 0)
            .copied()
            .collect();
        Delivery {
            proposal_dropped_for: dropped,
            precommit_dropped_for: BTreeSet::new(),
        }
    });

    match outcome {
        Ok(_) => Ok(()),
        Err(error) if error.to_string().contains("did not finalize within") => Ok(()),
        Err(error) => Err(format!(
            "seed {seed} (n={n}, f={f}, byzantine_indices={byzantine_indices:?}): {error}"
        )
        .into()),
    }
}

#[test]
fn property_safety_holds_across_random_schedules_n4_f1() -> TestResult {
    for seed in 0..SEED_COUNT_N4 {
        run_random_schedule(seed, 4, 1, ROUNDS_N4)?;
    }
    Ok(())
}

#[test]
fn property_safety_holds_across_random_schedules_n7_f2() -> TestResult {
    for seed in 0..SEED_COUNT_N7 {
        // Distinct seed space from the n=4 run above, so the two
        // tests never coincidentally replay the same schedule.
        run_random_schedule(seed.wrapping_add(1 << 40), 7, 2, ROUNDS_N7)?;
    }
    Ok(())
}
