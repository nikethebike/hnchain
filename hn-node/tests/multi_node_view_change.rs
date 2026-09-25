//! ADR-0037/ADR-0038's own real multi-process proof: three of four
//! validators, spawned as three separate real OS processes running the
//! compiled `hn-node` binary, booting from a real genesis file over
//! real `127.0.0.1` TCP sockets -- consensus voting and quorum happen
//! entirely through the network stack, never an in-process function
//! call between "nodes."
//!
//! Validator index 0 (this cluster's `round_proposer` at height 0,
//! round 0) is deliberately never started, simulating a dead/
//! unreachable leader. The three running validators hold 3 of 4 equal
//! shares of voting power (75%), comfortably above the `2f+1`
//! threshold, so this is a genuine partial-participation BFT case, not
//! a 3-of-3 test wearing a 4-validator label. Each running node still
//! lists the dead validator's address in its own `--peer` list (a real
//! deployment would not know in advance who is reachable) -- its
//! dialer thread simply never connects, harmlessly.
//!
//! Proves: round 0 never finalizes (the dead proposer's own round
//! produces nothing); every running process eventually finalizes height
//! 0 at round 1 (view change: propose-timeout, round advance, a
//! rotated -- and running -- proposer) with matching block/certificate
//! values across all three independently-driven processes; a second
//! height finalizes afterward, proving the chain continues past the
//! recovered round rather than stalling right after one view change.

mod support;

use std::time::Duration;

use support::{TestResult, block_field, spawn_node, wait_until_all_log, write_devnet_genesis};

const VALIDATOR_COUNT: u8 = 4;
const RUNNING_VALIDATORS: [u8; 3] = [1, 2, 3];
const BASE_PORT: u16 = 31_700;
const BASE_TIMEOUT_MS: u64 = 200;
const WAIT: Duration = Duration::from_secs(25);

#[test]
fn three_of_four_validators_survive_a_dead_round_zero_proposer() -> TestResult {
    let data_root =
        std::env::temp_dir().join(format!("hn-node-view-change-test-{}", std::process::id()));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;

    let mut nodes = Vec::new();
    for validator_index in RUNNING_VALIDATORS {
        let data_dir = data_root.join(format!("validator-{validator_index}"));
        std::fs::create_dir_all(&data_dir)?;
        nodes.push(spawn_node(
            &genesis_path,
            &data_dir,
            validator_index,
            BASE_PORT,
            &(0..VALIDATOR_COUNT).collect::<Vec<_>>(),
            BASE_TIMEOUT_MS,
        )?);
    }
    let node_refs: Vec<&support::NodeProcess> = nodes.iter().collect();

    // Height 0, round 1: the real view change. All three running
    // processes must independently reach it.
    let round1_lines = wait_until_all_log(
        &node_refs,
        |line| line.starts_with("FINALIZED height=0 round=1 "),
        WAIT,
    )?;

    // Every process's cross-process-built quorum certificate finalized
    // the exact same block -- real agreement, not a shared in-memory
    // value (these are three separate OS processes).
    let expected_block = block_field(&round1_lines[0])?.to_string();
    for line in &round1_lines[1..] {
        if block_field(line)? != expected_block {
            return Err("nodes finalized different blocks at height 0, round 1".into());
        }
    }

    // The dead proposer's own round must never have finalized anything.
    for node in &nodes {
        if node
            .log_snapshot()?
            .iter()
            .any(|line| line.starts_with("FINALIZED height=0 round=0 "))
        {
            return Err("height 0 round 0 finalized despite its proposer never running".into());
        }
    }

    // The chain continues past the recovered round, not just a
    // one-off recovery.
    wait_until_all_log(
        &node_refs,
        |line| line.starts_with("FINALIZED height=1 "),
        WAIT,
    )?;

    Ok(())
}
