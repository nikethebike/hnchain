//! Shared multi-process test support for `hn-node`'s own integration
//! tests (ADR-0037/ADR-0038/ADR-0039) — genesis-file construction and
//! real `std::process::Command`-spawned node processes, factored out
//! once both `multi_node_view_change.rs` and `restart_recovery.rs`
//! needed the identical devnet-genesis derivation. Not itself a test
//! (`tests/support/mod.rs` is never run by `cargo test` on its own —
//! the standard Rust convention for sharing code between integration
//! test binaries).
//!
//! Each integration test file compiles this module into its own,
//! separate binary, so a helper only one test file actually calls is
//! genuinely unused dead code *from that other binary's own point of
//! view* — an inherent property of sharing one module across several
//! independent crate roots this way, not a sign anything here is
//! actually unused overall.
#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hn_crypto::{Ed25519KeyPair, KeyRole, account_address_body};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub const NETWORK_ID: u16 = 1;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn listen_addr(base_port: u16, index: u8) -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        base_port + u16::from(index),
    )
}

/// Consensus key seed for devnet validator `index` -- matches
/// `hn-node/examples/print_devnet_genesis.rs`'s own convention exactly,
/// so a node configured with this seed resolves to `validator_id =
/// [index; 32]` in a genesis file built by [`write_devnet_genesis`].
pub fn consensus_key_seed(index: u8) -> [u8; 32] {
    [index; 32]
}

/// This node's own network/handshake key seed -- distinct from
/// `consensus_key_seed`'s own range (`0x80..` never collides with a
/// realistic devnet validator count's `0..` range).
pub fn network_key_seed(index: u8) -> [u8; 32] {
    [0x80 + index; 32]
}

/// Builds the same devnet genesis shape
/// `hn-node/examples/print_devnet_genesis.rs` produces (duplicated
/// rather than imported from it -- a test fixture, not library code)
/// and writes it to `path`.
pub fn write_devnet_genesis(path: &Path, validator_count: u8) -> TestResult<()> {
    let validators: Vec<serde_json::Value> = (0..validator_count)
        .map(|index| {
            let keypair =
                Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, consensus_key_seed(index));
            let descriptor = keypair.key_descriptor();
            serde_json::json!({
                "validator_id": hex(&[index; 32]),
                "consensus_key_algorithm_id": descriptor.algorithm_id(),
                "consensus_key_public_key": hex(&descriptor.public_key_bytes()),
                "bonded_stake": (hn_state::MINIMUM_VALIDATOR_BOND * 10).to_string(),
            })
        })
        .collect();

    let allocation = |seed: u8, amount: u128| -> TestResult<serde_json::Value> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32]);
        let descriptor = keypair.key_descriptor();
        let address = account_address_body(
            NETWORK_ID,
            descriptor.algorithm_id(),
            &descriptor.public_key_bytes(),
        )?;
        Ok(serde_json::json!({
            "address": hex(&address),
            "amount": amount.to_string(),
        }))
    };

    let genesis = serde_json::json!({
        "manifest_version": 1,
        "chain_id": hn_core::ChainId::HNCHAIN.get(),
        "network_id": NETWORK_ID,
        "genesis_time": 1_758_758_400_u64,
        "genesis_message": "hn-node integration test fixture - not for production use",
        "validators": validators,
        "allocations": {
            "reserve": allocation(0xA0, hn_state::RESERVE_ALLOCATION)?,
            "founder": allocation(0xA1, hn_state::FOUNDER_ALLOCATION)?,
            "community": allocation(0xA2, hn_state::COMMUNITY_ALLOCATION)?,
        },
    });

    std::fs::write(path, serde_json::to_vec_pretty(&genesis)?)?;
    Ok(())
}

pub struct NodeProcess {
    child: Child,
    log: Arc<Mutex<Vec<String>>>,
}

impl Drop for NodeProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl NodeProcess {
    /// Kills this process explicitly and waits for it to exit, ahead of
    /// (and distinct from) the `Drop` impl's own best-effort cleanup --
    /// callers that want to restart a validator under the exact same
    /// identity/data-dir need the OS to have actually released the
    /// listening port first, not just fired a kill signal.
    pub fn kill_and_wait(mut self) -> TestResult<()> {
        self.child.kill()?;
        self.child.wait()?;
        Ok(())
    }

    /// A snapshot of every line this process has logged so far.
    pub fn log_snapshot(&self) -> TestResult<Vec<String>> {
        Ok(self
            .log
            .lock()
            .map_err(|_| "node log mutex poisoned")?
            .clone())
    }
}

/// Spawns validator `validator_index` as a real child process running
/// the compiled `hn-node` binary, peered with every index in
/// `peer_indices` (excluding its own), all addressed via `base_port`
/// (ADR-0037's own `listen_addr` convention, test-local only -- real
/// `hn-node` config takes an explicit `--peer` per address, not a
/// shared base port).
pub fn spawn_node(
    genesis_path: &Path,
    data_dir: &Path,
    validator_index: u8,
    base_port: u16,
    peer_indices: &[u8],
    base_timeout_ms: u64,
) -> TestResult<NodeProcess> {
    let genesis_path = genesis_path.to_str().ok_or("non-UTF-8 genesis path")?;
    let data_dir = data_dir.to_str().ok_or("non-UTF-8 data dir path")?;
    let listen = listen_addr(base_port, validator_index).to_string();

    let mut args = vec![
        "--genesis".to_string(),
        genesis_path.to_string(),
        "--data-dir".to_string(),
        data_dir.to_string(),
        "--listen".to_string(),
        listen,
        "--consensus-key-seed".to_string(),
        hex(&consensus_key_seed(validator_index)),
        "--network-key-seed".to_string(),
        hex(&network_key_seed(validator_index)),
        "--base-timeout-ms".to_string(),
        base_timeout_ms.to_string(),
    ];
    for &peer_index in peer_indices {
        if peer_index != validator_index {
            args.push("--peer".to_string());
            args.push(listen_addr(base_port, peer_index).to_string());
        }
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_hn-node"))
        .args(args)
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
    // content is not part of any test's assertions.
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
pub fn wait_until_all_log(
    nodes: &[&NodeProcess],
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

pub fn block_field(finalized_line: &str) -> TestResult<&str> {
    finalized_line
        .split_whitespace()
        .find_map(|field| field.strip_prefix("block="))
        .ok_or_else(|| "FINALIZED line missing a block= field".into())
}

/// Extracts the numeric `height=` value from a `FINALIZED ...` log
/// line.
pub fn height_field(finalized_line: &str) -> TestResult<u64> {
    let raw = finalized_line
        .split_whitespace()
        .find_map(|field| field.strip_prefix("height="))
        .ok_or("FINALIZED line missing a height= field")?;
    Ok(raw.parse()?)
}
