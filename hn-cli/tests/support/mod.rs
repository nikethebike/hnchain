//! Shared multi-process test support for `hn-cli`'s own integration
//! tests (ADR-0047, "Conformance / Test Requirements": "Integration
//! (real processes)"). Spawns real `hn-node` validator processes and
//! runs the real compiled `hnchain` binary as a subprocess against
//! them -- the same "real processes, not simulated" standard
//! `hn-node`'s own integration tests already hold themselves to
//! (`hn-node/tests/support/mod.rs`), whose genesis-fixture shape this
//! duplicates rather than imports (a test fixture, not library code;
//! `hn-cli` cannot import another crate's `tests/` module anyway).
//!
//! `CARGO_BIN_EXE_hn-node` is not available here: Cargo only sets
//! `CARGO_BIN_EXE_<name>` for binaries of the package under test
//! itself, not a dependency's (confirmed empirically -- a direct
//! `env!("CARGO_BIN_EXE_hn-node")` probe fails to compile from this
//! crate). [`hn_node_binary_path`] instead locates it as a build
//! sibling of this crate's own `CARGO_BIN_EXE_hnchain`, which Cargo
//! does set, since `cargo test --workspace` always builds every
//! workspace member's binaries into the same `target/<profile>/`
//! directory regardless of which package's tests triggered the build.
#![allow(dead_code)]

use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use hn_crypto::{Ed25519KeyPair, KeyRole, account_address_body};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub const NETWORK_ID: u16 = 1;

/// The devnet genesis's own real Reserve allocation seed (matches
/// `hn-node/tests/support::reserve_keypair` exactly).
pub const RESERVE_SEED: [u8; 32] = [0xA0; 32];

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn listen_addr(base_port: u16, index: u8) -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        base_port + u16::from(index),
    )
}

fn consensus_key_seed(index: u8) -> [u8; 32] {
    [index; 32]
}

fn network_key_seed(index: u8) -> [u8; 32] {
    [0x80 + index; 32]
}

/// Writes a devnet genesis file with `validator_count` validators and
/// the real Reserve/Founder/Community allocations, in exactly the
/// shape `hn_node::GenesisManifest::load` requires (duplicated from
/// `hn-node/examples/print_devnet_genesis.rs`'s own convention).
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

    let allocation = |seed: [u8; 32], amount: u128| -> TestResult<serde_json::Value> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, seed);
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
        "genesis_message": "hn-cli integration test fixture - not for production use",
        "validators": validators,
        "allocations": {
            "reserve": allocation(RESERVE_SEED, hn_state::RESERVE_ALLOCATION)?,
            "founder": allocation([0xA1; 32], hn_state::FOUNDER_ALLOCATION)?,
            "community": allocation([0xA2; 32], hn_state::COMMUNITY_ALLOCATION)?,
        },
        "extra_data": "",
    });

    std::fs::write(path, serde_json::to_vec_pretty(&genesis)?)?;
    Ok(())
}

/// Writes a plain `hnchain-key-v1` key file for a known seed --
/// `key generate` only ever creates a fresh random key, so the
/// Reserve account (a fixed, known seed, not something any CLI
/// command can produce) is written directly, in exactly
/// `hn_cli::keyfile`'s own format.
pub fn write_key_file(path: &Path, seed: [u8; 32]) -> TestResult<()> {
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "hnchain-key-v1")?;
    writeln!(file, "role: account-signing")?;
    writeln!(file, "seed: {}", hex(&seed))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub struct NodeProcess {
    child: std::process::Child,
}

impl Drop for NodeProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn hn_node_binary_path() -> PathBuf {
    let hnchain = PathBuf::from(env!("CARGO_BIN_EXE_hnchain"));
    let dir = hnchain
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf);
    dir.join(format!("hn-node{}", std::env::consts::EXE_SUFFIX))
}

/// Spawns validator `validator_index` as a real `hn-node` child
/// process, peered with every other index in `0..peer_count`,
/// optionally with `--rpc-listen`.
pub fn spawn_node(
    genesis_path: &Path,
    data_dir: &Path,
    validator_index: u8,
    peer_count: u8,
    base_port: u16,
    base_timeout_ms: u64,
    rpc_listen: Option<SocketAddr>,
) -> TestResult<NodeProcess> {
    let genesis_path = genesis_path.to_str().ok_or("non-UTF-8 genesis path")?;
    let data_dir = data_dir.to_str().ok_or("non-UTF-8 data dir path")?;

    let mut args = vec![
        "--genesis".to_string(),
        genesis_path.to_string(),
        "--data-dir".to_string(),
        data_dir.to_string(),
        "--listen".to_string(),
        listen_addr(base_port, validator_index).to_string(),
        "--consensus-key-seed".to_string(),
        hex(&consensus_key_seed(validator_index)),
        "--network-key-seed".to_string(),
        hex(&network_key_seed(validator_index)),
        "--base-timeout-ms".to_string(),
        base_timeout_ms.to_string(),
    ];
    for peer_index in 0..peer_count {
        if peer_index != validator_index {
            args.push("--peer".to_string());
            args.push(listen_addr(base_port, peer_index).to_string());
        }
    }
    if let Some(rpc_listen) = rpc_listen {
        args.push("--rpc-listen".to_string());
        args.push(rpc_listen.to_string());
    }

    let child = Command::new(hn_node_binary_path())
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(NodeProcess { child })
}

/// Polls `GET {path}` on `addr` until `predicate` holds against the
/// decoded JSON body, or `timeout` elapses first.
pub fn wait_until_rpc(
    addr: SocketAddr,
    path: &str,
    predicate: impl Fn(&serde_json::Value) -> bool,
    timeout: Duration,
) -> TestResult<serde_json::Value> {
    let config = hn_rpc::ClientConfig {
        connect_timeout: Duration::from_secs(2),
        total_timeout: Duration::from_secs(2),
    };
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok((_, value)) = hn_rpc::get_json(addr, path, config)
            && predicate(&value)
        {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting on GET {path} against {addr}").into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Runs the real, compiled `hnchain` binary with `args` and returns
/// its captured output (`Command::output`'s own default: stdin is
/// `Stdio::null`, so a `send` without `--yes` would correctly refuse
/// rather than hang waiting for a confirmation that will never come).
pub fn run_hnchain(args: &[&str]) -> TestResult<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_hnchain"))
        .args(args)
        .output()?)
}

/// [`run_hnchain`]'s own stdout, as UTF-8 text (every `hnchain`
/// human-readable code path only ever writes ASCII/sanitized text).
pub fn run_hnchain_text(args: &[&str]) -> TestResult<(i32, String, String)> {
    let output = run_hnchain(args)?;
    let code = output.status.code().ok_or("process terminated by signal")?;
    Ok((
        code,
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?,
    ))
}

/// Extracts a `key generate`/`key show` text report's own `address:`
/// line.
pub fn extract_address(stdout: &str) -> TestResult<String> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("address:"))
        .map(|rest| rest.trim().to_string())
        .ok_or_else(|| "no address: line in hnchain output".into())
}
