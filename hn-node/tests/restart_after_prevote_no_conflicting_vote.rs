//! ADR-0048's own "Verification Requirements" item 4, its first named
//! case: "One real-process test: 4 processes, kill one right after it
//! prevoted, restart it, assert the network still finalizes and the
//! restarted node emits no conflicting vote (the node logs every
//! signed vote; the test compares them)."
//!
//! Four real validator processes run a real happy path first (the
//! same `restart_recovery.rs` pattern this file borrows directly).
//! Validator 3 is then killed (a real `SIGKILL`/process termination,
//! ADR-0038) the instant it logs its own very first signed vote for a
//! *fresh* height -- as close to "right after it prevoted" as a real
//! process boundary allows -- while the other three (75% of equal
//! voting power, comfortably above `2f+1`) keep finalizing well past
//! that point entirely without it. It is then restarted against its
//! own *same* `--data-dir` and identity and catches up (ADR-0044
//! sync) past where its peers already were.
//!
//! The actual safety check: every `(height, round, vote_type)` key
//! this validator's own log names *both* before it died and again
//! *after* it restarted must carry the exact same `target` -- the
//! vote journal (ADR-0048 rules 1 and 2) really restoring from a real
//! crash through a real file and a real process boundary, not just
//! the in-process `hn_consensus`/`hn_storage` tests already covering
//! the same property without any of that. Whether this restarted
//! validator's own journal resume (`resume_or_begin_round`) wins the
//! race against ADR-0044 sync for the exact round it died in, or sync
//! wins and replaces its engine before resuming ever gets a chance to
//! fire, is itself real, un-forced behavior this test observes rather
//! than engineers -- either way, nothing it signs after restarting may
//! ever contradict what it already signed before.

mod support;

use std::time::Duration;

use support::{
    NodeProcess, TestResult, height_field, spawn_node, voted_targets_by_key, wait_until_all_log,
    write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 33_300;
// Same reasoning (and the same real CI-observed need to widen it) as
// restart_recovery.rs's own identical constants.
const BASE_TIMEOUT_MS: u64 = 300;
const WAIT: Duration = Duration::from_secs(180);
const ALL_VALIDATORS: [u8; 4] = [0, 1, 2, 3];
const RESTARTED_VALIDATOR: u8 = 3;

#[test]
fn a_restarted_validator_never_signs_a_conflicting_vote() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-restart-after-prevote-test-{}",
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
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
        wait_until_all_log(
            &node_refs,
            |line| line.starts_with("FINALIZED height=3 "),
            WAIT,
        )?;
    }

    // As close to "right after it prevoted" as a real process boundary
    // allows: kill it the instant its own first vote for the next,
    // not-yet-finalized height appears in its own log.
    wait_until_all_log(
        &[&nodes[usize::from(RESTARTED_VALIDATOR)]],
        |line| line.starts_with("VOTED height=4 round=0 type=Prevote"),
        WAIT,
    )?;
    let killed = nodes.remove(usize::from(RESTARTED_VALIDATOR));
    let votes_before = voted_targets_by_key(&killed.log_snapshot()?)?;
    killed.kill_and_wait()?;

    // The remaining three keep finalizing well past where the network
    // was when it died -- proving the network's own progress is real
    // and ongoing while it is down, not paused waiting for it.
    let surviving_height = {
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
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

    // Restart it -- same genesis, same data dir, same identity, as a
    // brand new OS process.
    let restarted = spawn_node(
        &genesis_path,
        &data_dir(RESTARTED_VALIDATOR),
        RESTARTED_VALIDATOR,
        BASE_PORT,
        &ALL_VALIDATORS,
        BASE_TIMEOUT_MS,
    )?;
    nodes.push(restarted);
    let restarted_ref = nodes.last().ok_or("restarted node missing")?;

    // It must go on to independently finalize a height at or past
    // where its peers already were the moment it rejoined -- genuinely
    // caught up, not merely connected (same proof restart_recovery.rs
    // already establishes for ADR-0044 sync on its own).
    wait_until_all_log(
        &[restarted_ref],
        |line| {
            line.starts_with("FINALIZED height=")
                && height_field(line).is_ok_and(|height| height >= surviving_height)
        },
        WAIT,
    )?;

    let votes_after = voted_targets_by_key(&restarted_ref.log_snapshot()?)?;
    let mut conflict_checks = 0;
    for (key, target_before) in &votes_before {
        if let Some(target_after) = votes_after.get(key) {
            conflict_checks += 1;
            assert_eq!(
                target_after, target_before,
                "conflicting vote for (height, round, type) = {key:?}: \
                 before the crash it signed {target_before}, after restarting it signed {target_after}"
            );
        }
    }
    eprintln!(
        "restart_after_prevote_no_conflicting_vote: compared {conflict_checks} \
         (height, round, type) key(s) logged on both sides of the restart"
    );

    Ok(())
}
