//! ADR-0039's own real multi-process proof: the user's stated
//! acceptance criterion for "the first real multi-node devnet" --
//! "kill a node, bring it back up, it recovers the network's actual
//! current state by itself." Four validators run a real happy-path
//! network; one is killed mid-run (a real `SIGKILL`/process
//! termination, not a graceful shutdown -- this daemon installs no
//! signal handler at all, ADR-0038); the remaining three keep
//! finalizing without it (75% of equal voting power, comfortably above
//! `2f+1`); the killed validator is then restarted against its own
//! *same* `--data-dir` and identity, as a brand new OS process that
//! starts, like every process, at height 0 in memory.
//!
//! Proves real recovery, not just successful reconnection: the
//! restarted process reconnects to its former peers (ADR-0039's own
//! connection-drop redial -- nothing else would ever contact it again
//! otherwise), observes their real current height from ordinary
//! gossiped consensus traffic, jumps forward to it (at least one
//! `SYNCED` log line), and goes on to independently finalize a height
//! at or beyond where its peers already were at the moment it rejoined
//! -- genuinely caught up, not stuck replaying from height 0 forever
//! alongside a network that has long since moved on.

mod support;

use std::time::Duration;

use support::{TestResult, height_field, spawn_node, wait_until_all_log, write_devnet_genesis};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 31_800;
const BASE_TIMEOUT_MS: u64 = 150;
const WAIT: Duration = Duration::from_secs(25);
const ALL_VALIDATORS: [u8; 4] = [0, 1, 2, 3];
const RESTARTED_VALIDATOR: u8 = 3;

#[test]
fn a_killed_validator_restarts_and_catches_up_to_the_networks_real_height() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-restart-recovery-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;

    let data_dir = |index: u8| data_root.join(format!("validator-{index}"));

    let mut nodes = Vec::new();
    for validator_index in ALL_VALIDATORS {
        std::fs::create_dir_all(data_dir(validator_index))?;
        nodes.push(spawn_node(
            &genesis_path,
            &data_dir(validator_index),
            validator_index,
            BASE_PORT,
            &ALL_VALIDATORS,
            BASE_TIMEOUT_MS,
        )?);
    }

    // Full happy path first: all four agree on an early height before
    // anything gets killed.
    {
        let node_refs: Vec<&support::NodeProcess> = nodes.iter().collect();
        wait_until_all_log(
            &node_refs,
            |line| line.starts_with("FINALIZED height=3 "),
            WAIT,
        )?;
    }

    // Kill validator 3 for real, and wait for the OS to actually free
    // its listening port before moving on -- restarting it under the
    // exact same `--listen` address needs that, not just a fired kill
    // signal.
    let killed = nodes.remove(usize::from(RESTARTED_VALIDATOR));
    killed.kill_and_wait()?;

    // The remaining three keep finalizing well past where the network
    // was when validator 3 died -- proving the network's own progress
    // is real and ongoing while it is down, not paused waiting for it.
    let surviving_height = {
        let node_refs: Vec<&support::NodeProcess> = nodes.iter().collect();
        let lines = wait_until_all_log(
            &node_refs,
            |line| line.starts_with("FINALIZED height=15 "),
            WAIT,
        )?;
        lines
            .iter()
            .map(|line| height_field(line))
            .collect::<TestResult<Vec<_>>>()?
            .into_iter()
            .max()
            .ok_or("no surviving node logged a height")?
    };

    // Restart validator 3 -- same genesis, same data dir, same
    // identity, as a brand new OS process.
    let restarted = spawn_node(
        &genesis_path,
        &data_dir(RESTARTED_VALIDATOR),
        RESTARTED_VALIDATOR,
        BASE_PORT,
        &ALL_VALIDATORS,
        BASE_TIMEOUT_MS,
    )?;
    nodes.push(restarted);

    // It must observe the network's real height and jump forward, not
    // silently replay from height 0 forever.
    let restarted_ref = nodes.last().ok_or("restarted node missing")?;
    wait_until_all_log(
        &[restarted_ref],
        |line| line.starts_with("SYNCED to height="),
        WAIT,
    )?;

    // And it must go on to independently finalize a height at or past
    // where its peers already were the moment it rejoined -- genuinely
    // caught up, not merely connected.
    let caught_up_lines = wait_until_all_log(
        &[restarted_ref],
        |line| {
            line.starts_with("FINALIZED height=")
                && height_field(line).is_ok_and(|height| height >= surviving_height)
        },
        WAIT,
    )?;
    height_field(&caught_up_lines[0])?;

    Ok(())
}
