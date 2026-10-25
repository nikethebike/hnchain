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

use std::io::{BufRead, BufReader, Read};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hn_core::{
    AccountNonce, BlockHeight, Epoch, ProtocolEpoch, ProtocolVersion, Round, UnixTimeMillis,
};
use hn_crypto::{Digest, Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
use hn_network::{
    Channel, HandshakeAction, HandshakeEvent, HandshakeParams, HandshakeState, Hello, MessageType,
    P2PMessageEnvelopeV1, SyncBlockRangeRequestV1, SyncBlockRangeResponseV1, read_frame,
    write_frame,
};
use hn_state::{
    AccessListV1, BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader,
    ConsensusVote, FINALIZED_BLOCK_VERSION_1, FinalizedBlock, HEADER_VERSION_1, QuorumCertificate,
    TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType, ValidityWindowV1,
    VoteSigningPayloadV1, VoteTargetType, VoteType, list_empty_root,
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
    spawn_node_with_rpc_listen(
        genesis_path,
        data_dir,
        validator_index,
        base_port,
        peer_indices,
        base_timeout_ms,
        None,
    )
}

/// [`spawn_node`], additionally passing `--rpc-listen <rpc_listen>`
/// when `rpc_listen` is `Some` (ADR-0046, "RPC v1") — a separate
/// function rather than a new required parameter on `spawn_node`
/// itself, so every existing call site (none of which cares about RPC
/// at all) stays unchanged.
pub fn spawn_node_with_rpc_listen(
    genesis_path: &Path,
    data_dir: &Path,
    validator_index: u8,
    base_port: u16,
    peer_indices: &[u8],
    base_timeout_ms: u64,
    rpc_listen: Option<SocketAddr>,
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
    if let Some(rpc_listen) = rpc_listen {
        args.push("--rpc-listen".to_string());
        args.push(rpc_listen.to_string());
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

/// Extracts the numeric `height=` value from a `FINALIZED ...` (or
/// `SYNCED ...`) log line.
pub fn height_field(finalized_line: &str) -> TestResult<u64> {
    let raw = finalized_line
        .split_whitespace()
        .find_map(|field| field.strip_prefix("height="))
        .ok_or("FINALIZED line missing a height= field")?;
    Ok(raw.parse()?)
}

/// Whether `line` is either of the two ways a real node reports
/// having reached a given height: `"FINALIZED height=N ..."` (live
/// consensus, `act_on`) or `"SYNCED height=N ..."` (historical sync,
/// ADR-0044). A validator that happens to start up measurably later
/// than its peers can receive even its very first few heights only
/// through sync, never finalizing them live itself -- found via a
/// real CI failure, not simulated: a validator whose own log never
/// once said `"FINALIZED height=1 "` even after reaching height 560+,
/// because every one of its own early heights arrived as `SYNCED`
/// instead. A caller checking only for the `FINALIZED` line can wait
/// forever on such a node even though it has genuinely reached, and
/// moved past, that height.
pub fn is_height_report(line: &str) -> bool {
    line.starts_with("FINALIZED height=") || line.starts_with("SYNCED height=")
}

/// Whether `line` reports reaching exactly `height`, via either path
/// [`is_height_report`] recognizes.
pub fn reached_exact_height(line: &str, height: u64) -> bool {
    is_height_report(line) && height_field(line).ok() == Some(height)
}

/// Whether `line` reports reaching at least `height`, via either path
/// [`is_height_report`] recognizes.
pub fn reached_at_least_height(line: &str, height: u64) -> bool {
    is_height_report(line) && height_field(line).is_ok_and(|reached| reached >= height)
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
/// amount a test would plausibly use) to `recipient`, at `nonce` --
/// `bootstrap_key` is only present at `nonce == 0`: the Reserve
/// account's own identity is bootstrapped by its first-ever
/// transaction, and a later one claiming the key again would be
/// rejected (`UnexpectedBootstrapKey`) since the identity already
/// exists by then.
pub fn signed_reserve_transfer(
    recipient: Digest,
    amount: u128,
    nonce: u64,
) -> TestResult<TransactionEnvelope> {
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
        bootstrap_key: (nonce == 0).then(|| keypair.key_descriptor()),
        nonce: AccountNonce::new(nonce),
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

/// Spawns a background thread acting as a single dishonest peer at
/// `addr`: completes a real `Hello` handshake, immediately announces
/// `claimed_height` over one syntactically valid (but never
/// signature-checked at that stage, ADR-0044) `ConsensusVote` to make
/// itself the connection a real node's own `observe_height` picks as
/// the peer to ask first, then answers the resulting
/// `SyncBlockRangeRequest` with one forged `FinalizedBlock` --
/// self-consistent in isolation, but never actually finalized by
/// anyone's real `commit_qc`. A node's own verification
/// (`verify_and_commit_synced_block`) must reject it and move on to a
/// different, honest peer. Errors are swallowed (printed to stderr):
/// this runs detached in its own thread, with no result to propagate
/// back to the test:: a caller observes this peer's effect only
/// through the real node's own log lines (a `REJECTED synced block`
/// line, followed by eventually catching up regardless).
pub fn spawn_dishonest_sync_peer(
    addr: SocketAddr,
    chain_id: u8,
    network_id: u16,
    claimed_height: u64,
) -> TestResult<()> {
    // Bound synchronously, in the caller's own thread, before this
    // function returns: a real validator dialing this address must
    // never lose the race against this peer's own startup -- every
    // other peer it might dial is a real `hn-node` process whose
    // listener has already been up for a while, with no such
    // start-up delay of its own to compete against.
    let listener = TcpListener::bind(addr)?;
    std::thread::spawn(move || {
        if let Err(error) = run_dishonest_sync_peer(listener, chain_id, network_id, claimed_height)
        {
            eprintln!("dishonest sync peer exited: {error}");
        }
    });
    Ok(())
}

fn run_dishonest_sync_peer(
    listener: TcpListener,
    chain_id: u8,
    network_id: u16,
    claimed_height: u64,
) -> TestResult<()> {
    let (mut stream, _) = listener.accept()?;

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
    let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorNetwork, [0xDD; 32]);
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

    let request = read_frame(&mut stream)?;
    let peer_hello = Hello::decode(&request.payload)?;
    match handshake.apply(HandshakeEvent::ReceiveHello(peer_hello))? {
        HandshakeAction::Accepted { .. } => {}
        other => return Err(format!("handshake not accepted: {other:?}").into()),
    }

    let fake_vote = ConsensusVote {
        payload: VoteSigningPayloadV1 {
            vote_type: VoteType::Precommit,
            chain_id,
            network_id,
            epoch: Epoch::new(0),
            height: BlockHeight::new(claimed_height),
            round: Round::new(0),
            validator_set_commitment: [0x11; 32],
            validator_id: [0xDD; 32],
            target_type: VoteTargetType::Block,
            target_hash: [0xaa; 32],
            vote_metadata: Vec::new(),
        },
        signature: SignatureEnvelope {
            algorithm_id: 1,
            key_reference: None,
            signature: vec![0xaa; 64],
        },
    };
    send_envelope(
        &mut stream,
        chain_id,
        network_id,
        Channel::Consensus,
        MessageType::ConsensusVote,
        fake_vote.encode()?,
    )?;

    // Ignore anything that is not the `SyncBlockRangeRequest` this
    // whole setup exists to answer (a stray vote/proposal the real
    // node sends for some other, unrelated reason).
    let sync_request = loop {
        let frame = read_frame(&mut stream)?;
        if frame.channel == Channel::Sync
            && frame.message_type == MessageType::SyncBlockRangeRequest
        {
            break SyncBlockRangeRequestV1::decode(&frame.payload)?;
        }
    };

    let forged = forged_finalized_block(sync_request.from_height.get() + 1, chain_id, network_id)?;
    send_envelope(
        &mut stream,
        chain_id,
        network_id,
        Channel::Sync,
        MessageType::SyncBlockRangeResponse,
        SyncBlockRangeResponseV1 {
            blocks: vec![forged],
        }
        .encode()?,
    )?;

    // Dropping (and so closing) the stream immediately after this
    // final write races the OS's own transmission of it -- observed
    // in practice as the peer never receiving the response at all.
    // Reading until the peer closes its own end first (after it has
    // what it needs, it never sends anything else) waits for that
    // transmission to genuinely complete before this connection ends.
    let mut drain = Vec::new();
    let _ = stream.read_to_end(&mut drain);

    Ok(())
}

/// Spawns a background thread acting as a single peer at `addr`:
/// completes a real `Hello` handshake, sends `qc_payload` (expected to
/// be a real, validly-signed `QuorumCertificateMessage` the connecting
/// node has no cached proposal for), waits `delay_before_block`, then
/// sends `proposal_payload` (expected to be the real, validly-signed
/// `ConsensusProposal` matching that same certificate's own target) --
/// ADR-0048's own required proof that the old caller-side height/round/
/// step filter is really gone: a node must still finalize a precommit
/// certificate for a round it has already moved past, once the
/// matching block shows up afterward, not only when both arrive
/// already in order (R8's "decide at any round" plus its own
/// `NeedBlock`/`BlockFetched` retry). Errors are swallowed (printed to
/// stderr), same reasoning as [`spawn_dishonest_sync_peer`]: a caller
/// observes this peer's effect only through the real node's own log
/// lines.
pub fn spawn_precommit_then_block_peer(
    addr: SocketAddr,
    chain_id: u8,
    network_id: u16,
    qc_payload: Vec<u8>,
    delay_before_block: Duration,
    proposal_payload: Vec<u8>,
) -> TestResult<()> {
    let listener = TcpListener::bind(addr)?;
    std::thread::spawn(move || {
        if let Err(error) = run_precommit_then_block_peer(
            listener,
            chain_id,
            network_id,
            qc_payload,
            delay_before_block,
            proposal_payload,
        ) {
            eprintln!("precommit-then-block peer exited: {error}");
        }
    });
    Ok(())
}

fn run_precommit_then_block_peer(
    listener: TcpListener,
    chain_id: u8,
    network_id: u16,
    qc_payload: Vec<u8>,
    delay_before_block: Duration,
    proposal_payload: Vec<u8>,
) -> TestResult<()> {
    let (mut stream, _) = listener.accept()?;

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
    let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorNetwork, [0xCC; 32]);
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

    let request = read_frame(&mut stream)?;
    let peer_hello = Hello::decode(&request.payload)?;
    match handshake.apply(HandshakeEvent::ReceiveHello(peer_hello))? {
        HandshakeAction::Accepted { .. } => {}
        other => return Err(format!("handshake not accepted: {other:?}").into()),
    }

    send_envelope(
        &mut stream,
        chain_id,
        network_id,
        Channel::Consensus,
        MessageType::QuorumCertificateMessage,
        qc_payload,
    )?;
    std::thread::sleep(delay_before_block);
    send_envelope(
        &mut stream,
        chain_id,
        network_id,
        Channel::Consensus,
        MessageType::ConsensusProposal,
        proposal_payload,
    )?;

    let mut drain = Vec::new();
    let _ = stream.read_to_end(&mut drain);

    Ok(())
}

/// A self-consistent (internally) but never-really-finalized
/// `FinalizedBlock` at `height` -- `commit_qc` carries a well-formed
/// but entirely fabricated signature, matching no real validator's
/// own key; a real node's `verify_and_commit_synced_block` must
/// reject this regardless of which specific check catches it first.
fn forged_finalized_block(
    height: u64,
    chain_id: u8,
    network_id: u16,
) -> TestResult<FinalizedBlock> {
    let body = BlockBody {
        body_version: BODY_VERSION_1,
        transactions: Vec::new(),
        receipts: Vec::new(),
        evidence: Vec::new(),
        extra_data: Vec::new(),
    };
    let header = BlockHeader {
        header_version: HEADER_VERSION_1,
        chain_id,
        network_id,
        height: BlockHeight::new(height),
        round: Round::new(0),
        epoch: Epoch::new(0),
        protocol_epoch: ProtocolEpoch::GENESIS,
        parent_block_hash: [0_u8; 32],
        proposer: [0xdd_u8; 32],
        timestamp: UnixTimeMillis::from_millis(height),
        transactions_root: body.transactions_root()?,
        state_root: [0xee_u8; 32],
        receipts_root: body.receipts_root()?,
        events_root: list_empty_root()?,
        consensus_root: [0x11_u8; 32],
        evidence_root: body.evidence_root()?,
        protocol_parameters_hash: [0x04_u8; 32],
        extra_data_hash: body.extra_data_hash()?,
    };
    let envelope = BlockEnvelope {
        block_version: BLOCK_VERSION_1,
        header,
        body,
        justification: None,
    };
    let block_hash = envelope.block_hash()?;
    let commit_qc = QuorumCertificate {
        certificate_type: VoteType::Precommit,
        chain_id,
        network_id,
        epoch: Epoch::new(0),
        height: BlockHeight::new(height),
        round: Round::new(0),
        validator_set_commitment: [0x11_u8; 32],
        target_type: VoteTargetType::Block,
        target_hash: block_hash,
        total_voting_power: 100,
        signed_voting_power: 100,
        signer_commitment: vec![0b0000_0001],
        aggregate_proof: vec![SignatureEnvelope {
            algorithm_id: 1,
            key_reference: None,
            signature: vec![0xaa; 64],
        }],
    };
    Ok(FinalizedBlock {
        finalized_block_version: FINALIZED_BLOCK_VERSION_1,
        envelope,
        commit_qc,
    })
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

/// This test module's own fixed deadlines for [`http_rpc_client_config`]
/// -- generous enough that a genuinely slow devnet process never trips
/// them, short enough that a real hang (the bug, not the test) still
/// fails in seconds rather than hanging the whole suite.
fn http_rpc_client_config() -> hn_rpc::ClientConfig {
    hn_rpc::ClientConfig {
        connect_timeout: Duration::from_secs(5),
        total_timeout: Duration::from_secs(5),
    }
}

/// Sends one bare HTTP/1.1 request to `addr` (ADR-0046, "RPC v1") and
/// returns its status code and raw body bytes — `hn_rpc`'s own client
/// (ADR-0047, "Decided: HTTP Client Lives In `hn-rpc`"), shared with
/// `hn-cli` rather than this test module hand-rolling its own copy.
pub fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> TestResult<(u16, Vec<u8>)> {
    Ok(hn_rpc::request(
        addr,
        method,
        path,
        body,
        http_rpc_client_config(),
    )?)
}

/// [`http_request`], decoding the response body as JSON.
pub fn http_get_json(addr: SocketAddr, path: &str) -> TestResult<(u16, serde_json::Value)> {
    Ok(hn_rpc::get_json(addr, path, http_rpc_client_config())?)
}

/// [`http_request`], encoding `payload` as the JSON request body and
/// decoding the response body as JSON.
pub fn http_post_json(
    addr: SocketAddr,
    path: &str,
    payload: &serde_json::Value,
) -> TestResult<(u16, serde_json::Value)> {
    Ok(hn_rpc::post_json(
        addr,
        path,
        payload,
        http_rpc_client_config(),
    )?)
}

/// Polls `GET {path}` on `addr` until `predicate` holds against the
/// decoded JSON body, or `timeout` elapses first — a connection
/// failure (the RPC server not up yet) is treated the same as "not
/// satisfied yet," not an error, since callers typically start polling
/// right after spawning the process.
pub fn wait_until_rpc(
    addr: SocketAddr,
    path: &str,
    predicate: impl Fn(&serde_json::Value) -> bool,
    timeout: Duration,
) -> TestResult<serde_json::Value> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok((_, value)) = http_get_json(addr, path)
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
