//! ADR-0037's own real multi-process proof: three of four validators,
//! spawned as three separate real OS processes running the compiled
//! `hn-node` binary, connected over real `127.0.0.1` TCP sockets --
//! consensus voting and quorum happen entirely through the network
//! stack, never an in-process function call between "nodes."
//!
//! Validator index 0 (this cluster's `round_proposer` at height 0,
//! round 0) is deliberately never started, simulating a dead/
//! unreachable leader. The three running validators hold 3 of 4 equal
//! shares of voting power (75%), comfortably above the `2f+1`
//! threshold, so this is a genuine partial-participation BFT case, not
//! a 3-of-3 test wearing a 4-validator label.
//!
//! Proves: round 0 never finalizes (the dead proposer's own round
//! produces nothing); every running process eventually finalizes height
//! 0 at round 1 (view change: propose-timeout, round advance, a
//! rotated -- and running -- proposer) with matching block/certificate
//! values across all three independently-driven processes; a second
//! height finalizes afterward, proving the chain continues past the
//! recovered round rather than stalling right after one view change.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const VALIDATOR_COUNT: u8 = 4;
const RUNNING_VALIDATORS: [u8; 3] = [1, 2, 3];
const BASE_PORT: u16 = 31_700;
const BASE_TIMEOUT_MS: u64 = 200;
const WAIT: Duration = Duration::from_secs(25);

struct NodeProcess {
    child: Child,
    log: Arc<Mutex<Vec<String>>>,
}

impl Drop for NodeProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_node(validator_index: u8, data_dir: &Path) -> TestResult<NodeProcess> {
    let data_dir = data_dir.to_str().ok_or("non-UTF-8 data dir path")?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_hn-node"))
        .args([
            "--validator-index",
            &validator_index.to_string(),
            "--validator-count",
            &VALIDATOR_COUNT.to_string(),
            "--base-port",
            &BASE_PORT.to_string(),
            "--data-dir",
            data_dir,
            "--base-timeout-ms",
            &BASE_TIMEOUT_MS.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().ok_or("child has no piped stdout")?;
    let stderr = child.stderr.take().ok_or("child has no piped stderr")?;
    let log = Arc::new(Mutex::new(Vec::new()));

    let reader_log = Arc::clone(&log);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(mut guard) = reader_log.lock() else {
                break;
            };
            guard.push(line);
        }
    });
    // Drained so the child never blocks on a full stderr pipe; its
    // content is not part of this test's assertions.
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            if line.is_err() {
                break;
            }
        }
    });

    Ok(NodeProcess { child, log })
}

/// Blocks until every node in `nodes` has logged at least one line for
/// which `predicate` holds, returning each node's first matching line
/// (same order as `nodes`) -- or an error once `timeout` elapses first.
fn wait_until_all_log(
    nodes: &[NodeProcess],
    predicate: impl Fn(&str) -> bool,
    timeout: Duration,
) -> TestResult<Vec<String>> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut matches = Vec::with_capacity(nodes.len());
        for node in nodes {
            let guard = node.log.lock().map_err(|_| "node log mutex poisoned")?;
            match guard.iter().find(|line| predicate(line)) {
                Some(line) => matches.push(line.clone()),
                None => break,
            }
        }
        if matches.len() == nodes.len() {
            return Ok(matches);
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for all nodes to log a matching line".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn block_field(finalized_line: &str) -> TestResult<&str> {
    finalized_line
        .split_whitespace()
        .find_map(|field| field.strip_prefix("block="))
        .ok_or_else(|| "FINALIZED line missing a block= field".into())
}

#[test]
fn three_of_four_validators_survive_a_dead_round_zero_proposer() -> TestResult {
    let data_root =
        std::env::temp_dir().join(format!("hn-node-view-change-test-{}", std::process::id()));
    std::fs::create_dir_all(&data_root)?;

    let mut nodes = Vec::new();
    for validator_index in RUNNING_VALIDATORS {
        let data_dir = data_root.join(format!("validator-{validator_index}"));
        std::fs::create_dir_all(&data_dir)?;
        nodes.push(spawn_node(validator_index, &data_dir)?);
    }

    // Height 0, round 1: the real view change. All three running
    // processes must independently reach it.
    let round1_lines = wait_until_all_log(
        &nodes,
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
        let guard = node.log.lock().map_err(|_| "node log mutex poisoned")?;
        if guard
            .iter()
            .any(|line| line.starts_with("FINALIZED height=0 round=0 "))
        {
            return Err("height 0 round 0 finalized despite its proposer never running".into());
        }
    }

    // The chain continues past the recovered round, not just a
    // one-off recovery.
    wait_until_all_log(&nodes, |line| line.starts_with("FINALIZED height=1 "), WAIT)?;

    Ok(())
}
