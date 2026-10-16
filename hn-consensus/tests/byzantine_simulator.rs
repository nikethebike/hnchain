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
