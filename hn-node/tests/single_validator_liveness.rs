//! The real, independent bug `rpc_listen.rs`/`rpc_end_to_end.rs`/
//! `rpc_load.rs` all name in their own module docs and sidestep by
//! using 2+ validators: a lone validator holds its own entire voting
//! power, so it finalizes every height via `act_on`'s own self-
//! propose/self-vote/self-finalize chain with no network round-trip
//! ever required — and `act_on` used to call itself again directly
//! for each step's own follow-up action, so that chain recursed once
//! per height with no bound at all and overflowed the stack within a
//! few seconds.
//!
//! Without this test, the first-ever single-validator devnet a real
//! CLI user starts (the simplest possible way to try this node at
//! all) would crash almost immediately. A real process, run until it
//! finalizes a real height well past where the old recursive version
//! reliably crashed, is the only check that actually proves the fix
//! (`act_on`'s own trampoline loop, ADR-0046's own session) rather
//! than just reasoning about it.

mod support;

use std::time::Duration;

use support::{TestResult, height_field, spawn_node, wait_until_all_log, write_devnet_genesis};

const VALIDATOR_COUNT: u8 = 1;
const BASE_PORT: u16 = 33_000;
const BASE_TIMEOUT_MS: u64 = 5;
const TARGET_HEIGHT: u64 = 1000;
const WAIT: Duration = Duration::from_secs(120);

#[test]
fn a_single_validator_reaches_a_high_height_without_overflowing_its_stack() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-single-validator-liveness-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;
    let data_dir = data_root.join("validator-0");
    std::fs::create_dir_all(&data_dir)?;

    // No peers at all -- this validator holds 100% of the voting
    // power and must reach quorum, and keep reaching it height after
    // height, entirely on its own.
    let node = spawn_node(
        &genesis_path,
        &data_dir,
        0,
        BASE_PORT,
        &[0],
        BASE_TIMEOUT_MS,
    )?;

    let lines = wait_until_all_log(
        &[&node],
        |line| {
            line.starts_with("FINALIZED height=")
                && height_field(line).is_ok_and(|height| height >= TARGET_HEIGHT)
        },
        WAIT,
    )?;
    let reached = height_field(&lines[0])?;
    assert!(reached >= TARGET_HEIGHT, "only reached height {reached}");

    // Still alive and still answering, not a crashed process some
    // earlier log line happened to be flushed before dying.
    let log_len_before = node.log_snapshot()?.len();
    std::thread::sleep(Duration::from_millis(500));
    let log_len_after = node.log_snapshot()?.len();
    assert!(
        log_len_after > log_len_before,
        "process appears to have stopped producing output after reaching height {reached}"
    );

    Ok(())
}
