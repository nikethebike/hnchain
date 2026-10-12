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
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hn_core::{AccountNonce, ProtocolVersion};
use hn_crypto::{Digest, Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
use hn_network::{
    Channel, HandshakeAction, HandshakeEvent, HandshakeParams, HandshakeState, Hello, MessageType,
    P2PMessageEnvelopeV1, read_frame, write_frame,
};
use hn_state::{
    AccessListV1, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType, ValidityWindowV1,
};

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
        "extra_data": "",
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
    // Merged into the same log (tagged) rather than discarded: a panic
    // message -- the one thing most likely to explain a node that goes
    // silent on stdout -- prints to stderr, and `wait_until_all_log`'s
    // own timeout diagnostic needs to see it, not just the last stdout
    // lines.
    let stderr_log = Arc::clone(&log);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let Ok(mut guard) = stderr_log.lock() else {
                break;
            };
            guard.push(format!("[stderr] {line}"));
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
            let mut diagnostic =
                String::from("timed out waiting for all nodes to log a matching line:\n");
            for (index, node) in nodes.iter().enumerate() {
                let log = node.log_snapshot()?;
                let tail_start = log.len().saturating_sub(10);
                diagnostic.push_str(&format!(
                    "  node[{index}] logged {} lines, last {}:\n",
                    log.len(),
                    log.len() - tail_start
                ));
                for line in &log[tail_start..] {
                    diagnostic.push_str(&format!("    {line}\n"));
                }
            }
            return Err(diagnostic.into());
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

/// Extracts the hex `state_root=` value from a `FINALIZED ...` log
/// line (ADR-0045, "Decided: Validator Execution" -- every node
/// recomputes this independently after committing, so comparing it
/// across nodes is a real cross-node agreement check, not an assumed
/// one).
pub fn state_root_field(finalized_line: &str) -> TestResult<&str> {
    finalized_line
        .split_whitespace()
        .find_map(|field| field.strip_prefix("state_root="))
        .ok_or_else(|| "FINALIZED line missing a state_root= field".into())
}

/// The devnet genesis's own real Reserve allocation account (seed
/// `[0xA0; 32]`, matching `write_devnet_genesis`'s own convention
/// exactly) -- a real, known keypair, not a separately-seeded fixture
/// account.
pub fn reserve_keypair() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0xA0; 32])
}

/// A real, correctly-signed native transfer of `amount` from the
/// devnet genesis's own Reserve account (comfortably funded above any
/// amount a test would plausibly use) to `recipient`, at nonce 0 --
/// only valid as the *first* transaction the Reserve account's own
/// tests ever apply, since it bootstraps that account's identity.
pub fn signed_reserve_transfer(recipient: Digest, amount: u128) -> TestResult<TransactionEnvelope> {
    let keypair = reserve_keypair();
    let sender = account_address_body(
        NETWORK_ID,
        keypair.key_descriptor().algorithm_id(),
        &keypair.key_descriptor().public_key_bytes(),
    )?;
    let payload = TransferPayloadV1 {
        recipient,
        asset_id: None,
        amount,
    }
    .encode()?;
    let mut envelope = TransactionEnvelope {
        tx_version: TX_VERSION_1,
        chain_id: hn_core::ChainId::HNCHAIN.get(),
        network_id: NETWORK_ID,
        tx_type: TxType::Transfer,
        sender,
        bootstrap_key: Some(keypair.key_descriptor()),
        nonce: AccountNonce::new(0),
        fee_limit: 100,
        validity_window: ValidityWindowV1 {
            min_height: None,
            max_height: None,
        },
        access_list: AccessListV1 {
            reads: vec![],
            writes: vec![],
        },
        payload,
        signatures: vec![],
    };
    let digest = envelope.signing_payload().signing_digest()?;
    envelope.signatures = vec![SignatureEnvelope {
        algorithm_id: keypair.key_descriptor().algorithm_id(),
        key_reference: None,
        signature: keypair.sign(&digest).to_vec(),
    }];
    Ok(envelope)
}

/// This test-local protocol version -- matches `hn-node`'s own private
/// `PROTOCOL_VERSION` constant by value (not importable: it is not
/// `pub`, the same reasoning every other devnet fixture here duplicates
/// rather than reaches into the real binary's own internals for).
fn test_protocol_version() -> ProtocolVersion {
    ProtocolVersion::new(0, 1, 0)
}

/// Connects to `addr`, completes the real `Hello` handshake as a
/// minimal, non-validating peer (ADR-0045, "Decided: Devnet
/// Transaction Entry Point"), sends exactly one application message on
/// `channel`/`message_type`, and returns the still-open stream so a
/// caller can read a reply, if one is expected.
///
/// Declares the exact same `required_channels`/`required_message_types`
/// a real `hn-node` validator declares (`Channel::Handshake`/
/// `Consensus`; `Hello`/`ConsensusProposal`/`ConsensusVote`/
/// `QuorumCertificateMessage`) -- not because this client ever sends
/// any of those, but because that is what a real node's own handshake
/// requires its peer's `Hello` to claim support for today. A real
/// "non-validating full node" capability profile remains a distinct,
/// separate, still-open item (ADR-0038) this test-only client stands
/// in for, not an implementation of.
pub fn connect_handshake_and_send(
    addr: SocketAddr,
    chain_id: u8,
    network_id: u16,
    channel: Channel,
    message_type: MessageType,
    payload: Vec<u8>,
) -> TestResult<TcpStream> {
    let mut stream = TcpStream::connect(addr)?;

    let params = HandshakeParams {
        protocol_version: test_protocol_version(),
        chain_id,
        network_id,
        required_channels: vec![Channel::Handshake, Channel::Consensus],
        required_message_types: vec![
            MessageType::Hello,
            MessageType::ConsensusProposal,
            MessageType::ConsensusVote,
            MessageType::QuorumCertificateMessage,
        ],
    };
    let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorNetwork, [0xEE; 32]);
    let mut handshake = HandshakeState::new(params, keypair);

    let HandshakeAction::Send(hello) = handshake.apply(HandshakeEvent::SendHello)? else {
        return Err("expected to send a Hello first".into());
    };
    send_envelope(
        &mut stream,
        chain_id,
        network_id,
        Channel::Handshake,
        MessageType::Hello,
        hello.encode()?,
    )?;

    let response = read_frame(&mut stream)?;
    let peer_hello = Hello::decode(&response.payload)?;
    match handshake.apply(HandshakeEvent::ReceiveHello(peer_hello))? {
        HandshakeAction::Accepted { .. } => {}
        other => return Err(format!("handshake not accepted: {other:?}").into()),
    }

    send_envelope(
        &mut stream,
        chain_id,
        network_id,
        channel,
        message_type,
        payload,
    )?;
    Ok(stream)
}

fn send_envelope(
    stream: &mut TcpStream,
    chain_id: u8,
    network_id: u16,
    channel: Channel,
    message_type: MessageType,
    payload: Vec<u8>,
) -> TestResult<()> {
    let envelope = P2PMessageEnvelopeV1::new(
        test_protocol_version(),
        chain_id,
        network_id,
        channel,
        message_type,
        payload,
    )?;
    write_frame(stream, &envelope)?;
    Ok(())
}
