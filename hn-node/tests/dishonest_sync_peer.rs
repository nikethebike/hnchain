//! ADR-0044's own negative proof: a dishonest peer serving a forged
//! block during historical sync does not derail a node's own catch-up
//! -- `verify_and_commit_synced_block` rejects it (a logged, non-fatal
//! event, ADR-0044's own "Decided: Verification Procedure"), and the
//! node moves on to a different, honest peer and still reaches the
//! network's own real height.
//!
//! Run in two deliberately isolated phases, not as one single process
//! racing a real network against a scripted peer: a late validator
//! configured with *only* the dishonest peer as its own dial target
//! can only ever talk to it (no real validator is configured to dial
//! *it* either, so nothing else can race in and "accidentally" answer
//! first) -- a fully deterministic proof of rejection. It is then
//! killed and restarted against its own same `--data-dir` with the
//! dishonest peer swapped out for the three real validators, proving
//! the "switches to a different peer and still catches up" half the
//! same way `restart_recovery.rs`'s own test already does.

mod support;

use std::time::Duration;

use support::{
    NodeProcess, TestResult, height_field, reached_at_least_height, spawn_dishonest_sync_peer,
    spawn_node, wait_until_all_log, write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 32_300;
// Loosened from an earlier 150ms/90s (same reasoning as
// transaction_finalization.rs and friends: real validator processes
// under a CPU-starved CI runner can legitimately miss a tight round
// timeout, which only delays real progress further by forcing a
// view-change rather than tolerating the slowness).
const BASE_TIMEOUT_MS: u64 = 300;
const WAIT: Duration = Duration::from_secs(180);
// The late validator is genesis validator *0* deliberately -- `hn-node`
// dials by directed listen-address ordering ("only the lower address
// dials"), so giving it the lowest index/address of the two peers it
// might have (the dishonest one in phase 1, the three real ones in
// phase 2) makes *it* the one that actively dials, rather than
// passively waiting to be dialed.
const LATE_VALIDATOR: u8 = 0;
const HONEST_VALIDATORS: [u8; 3] = [1, 2, 3];
const DISHONEST_PEER_INDEX: u8 = 4;
const MINIMUM_SURVIVING_HEIGHT: u64 = 10;

#[test]
fn a_node_rejects_a_forged_block_switches_peers_and_still_catches_up() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-dishonest-sync-peer-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;
    let data_dir = |index: u8| data_root.join(format!("validator-{index}"));
    std::fs::create_dir_all(data_dir(LATE_VALIDATOR))?;

    // Phase 1: the late validator's own --peer list names *only* the
    // dishonest peer below, and (this is the point) no real validator
    // is started yet at all -- there is nothing else this process
    // could possibly connect to, so whatever it does, it does against
    // the dishonest peer specifically, deterministically.
    let dishonest_addr = support::listen_addr(BASE_PORT, DISHONEST_PEER_INDEX);
    spawn_dishonest_sync_peer(
        dishonest_addr,
        hn_core::ChainId::HNCHAIN.get(),
        support::NETWORK_ID,
        MINIMUM_SURVIVING_HEIGHT + 100,
    )?;
    let probe = spawn_node(
        &genesis_path,
        &data_dir(LATE_VALIDATOR),
        LATE_VALIDATOR,
        BASE_PORT,
        &[LATE_VALIDATOR, DISHONEST_PEER_INDEX],
        BASE_TIMEOUT_MS,
    )?;
    wait_until_all_log(
        &[&probe],
        |line| line.starts_with("REJECTED synced block"),
        WAIT,
    )?;
    // The forged block was never committed, so this process has
    // nothing durable beyond genesis to lose -- a plain kill (not a
    // graceful shutdown; this daemon installs no signal handler,
    // ADR-0038) is enough, same as `restart_recovery.rs`'s own test.
    probe.kill_and_wait()?;

    // Phase 2: three real validators, run well past ten blocks
    // entirely on their own, so the late validator -- restarted next,
    // genesis-only -- has a real, non-trivial gap to actually sync
    // across. They never need the late validator's own index in their
    // own peer list: its address is the lowest of all four, so
    // directed dial ordering already makes *it* responsible for
    // dialing *them*.
    let mut nodes = Vec::new();
    for validator_index in HONEST_VALIDATORS {
        std::fs::create_dir_all(data_dir(validator_index))?;
        nodes.push(spawn_node(
            &genesis_path,
            &data_dir(validator_index),
            validator_index,
            BASE_PORT,
            &HONEST_VALIDATORS,
            BASE_TIMEOUT_MS,
        )?);
    }
    let surviving_height = {
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
        let lines = wait_until_all_log(
            &node_refs,
            |line| reached_at_least_height(line, MINIMUM_SURVIVING_HEIGHT),
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
    assert!(surviving_height >= MINIMUM_SURVIVING_HEIGHT);

    // Restart the late validator -- same genesis, same data dir, same
    // identity -- now dialing the three real validators instead of
    // the dishonest peer.
    let late_node = spawn_node(
        &genesis_path,
        &data_dir(LATE_VALIDATOR),
        LATE_VALIDATOR,
        BASE_PORT,
        &[LATE_VALIDATOR, 1, 2, 3],
        BASE_TIMEOUT_MS,
    )?;
    nodes.push(late_node);
    let late_node_ref = nodes.last().ok_or("late node missing")?;

    // It must go on to really catch up, proving the earlier rejection
    // (a different peer, a different process, but the exact same
    // validator identity and data dir) was not fatal to ever syncing
    // at all.
    let caught_up_lines = wait_until_all_log(
        &[late_node_ref],
        |line| reached_at_least_height(line, surviving_height),
        WAIT,
    )?;
    height_field(&caught_up_lines[0])?;

    Ok(())
}
