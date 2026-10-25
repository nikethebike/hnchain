//! ADR-0048's own "Verification Requirements" item 4, its second named
//! case: "truncate the block store under a valid journal and assert
//! the fatal exit of rule 3."
//!
//! A single validator (its own 100% voting power, no peers, the same
//! pattern `single_validator_liveness.rs` already establishes) runs
//! far enough to produce a real vote journal reflecting real progress,
//! is killed, and has its entire block store deleted -- the most
//! extreme version of "the store lost data this validator already
//! voted on," leaving `latest_finalized_block()` at nothing while the
//! untouched vote journal still names a much later height. Restarting
//! against the same `--data-dir` and identity must hit rule 3's fatal
//! case: a logged `FATAL:` line and exit code 78
//! (`hn-node`'s own `JOURNAL_INCONSISTENT_EXIT_CODE`), never a
//! silently-resumed or silently-reset validator.

mod support;

use std::time::Duration;

use support::{TestResult, spawn_node, wait_until_all_log, write_devnet_genesis};

const VALIDATOR_COUNT: u8 = 1;
const BASE_PORT: u16 = 33_100;
const BASE_TIMEOUT_MS: u64 = 50;
const WAIT: Duration = Duration::from_secs(60);
// Matches `hn-node`'s own private `JOURNAL_INCONSISTENT_EXIT_CODE`
// constant by value (not importable: not `pub`, the same "duplicated
// rather than reaching into the real binary's own internals" reasoning
// every other devnet fixture in `support` already follows) -- ADR-0048
// rule 3's own "the value is fixed in implementation and documented."
const JOURNAL_INCONSISTENT_EXIT_CODE: i32 = 78;

#[test]
fn a_restarted_validator_exits_fatally_when_its_block_store_lost_data() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-journal-store-mismatch-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;
    let data_dir = data_root.join("validator-0");
    std::fs::create_dir_all(&data_dir)?;

    let node = spawn_node(
        &genesis_path,
        &data_dir,
        0,
        BASE_PORT,
        &[0],
        BASE_TIMEOUT_MS,
    )?;
    wait_until_all_log(
        &[&node],
        |line| line.starts_with("FINALIZED height=3 "),
        WAIT,
    )?;
    // Real OS-level kill, same as every other crash-recovery test here
    // -- this daemon installs no signal handler (ADR-0038), so a plain
    // kill is the only "it stopped" this test needs to simulate.
    node.kill_and_wait()?;

    std::fs::remove_file(data_dir.join("state.redb"))?;

    let mut restarted = spawn_node(
        &genesis_path,
        &data_dir,
        0,
        BASE_PORT,
        &[0],
        BASE_TIMEOUT_MS,
    )?;
    let exit_code = restarted.wait_for_exit(WAIT)?;
    let log = restarted.log_snapshot()?;
    assert_eq!(
        exit_code, JOURNAL_INCONSISTENT_EXIT_CODE,
        "expected the dedicated fatal exit code, log: {log:?}"
    );
    assert!(
        log.iter().any(|line| line.starts_with("FATAL:")),
        "expected a FATAL: log line, got: {log:?}"
    );

    Ok(())
}
