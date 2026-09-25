use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use hn_consensus::{ConsensusAction, ConsensusEngine, ConsensusTarget, round_proposer};
use hn_core::{BlockHeight, Epoch, ProtocolVersion, Round};
use hn_crypto::{Digest, Ed25519KeyPair, KeyRole, SignatureEnvelope, hash_profile_0x0001};
use hn_network::{
    Channel, ConsensusProposalMessageV1, HandshakeAction, HandshakeEvent, HandshakeParams,
    HandshakeState, Hello, MessageType, P2PMessageEnvelopeV1, PeerLink, spawn_peer_link,
};
use hn_state::{
    ConsensusVote, QuorumCertificate, StateCommitter, StateReader, ValidatorRecordV1,
    ValidatorStatus, VoteSigningPayloadV1, VoteTargetType, VoteType, Write, active_set,
};
use hn_storage::RedbStateStore;

use crate::config::NodeConfig;
use crate::error::{NodeError, NodeResult};
use crate::genesis::GenesisManifest;
use crate::identity::{keypair_from_seed, resolve_own_validator_id};

/// A fixed devnet `validator_set_commitment` (ADR-0037's own devnet
/// scope never defines `ValidatorSetCommitmentV1`'s real canonical
/// bytes — matches the established test convention throughout this
/// codebase of a fixed placeholder constant; nothing here cross-checks
/// it against a computed value).
const DEVNET_VSC: [u8; 32] = [0x11; 32];

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(0, 1, 0);

/// This node's own out-of-band genesis integrity marker key (ADR-0038,
/// "Decided: DB Init") — deliberately outside ADR-0007's own domain
/// registry: node-local bookkeeping, never a consensus-visible state
/// key.
fn genesis_marker_key() -> NodeResult<Digest> {
    Ok(hash_profile_0x0001("hnchain.node.genesismarker.v1", &[])?)
}

/// What the driving loop reacts to (ADR-0037, "Decided: Round Timer /
/// Driving Loop"; ADR-0039, "Decided: Connection-Drop Reconnection").
enum NodeEvent {
    NewConnection {
        conn_id: u64,
        link: PeerLink,
        /// `Some(addr)` if this node dialed `addr` to establish this
        /// connection — the address to redial if it later drops.
        /// `None` for an accepted (inbound) connection: reconnecting a
        /// dropped inbound link is the dialing side's own
        /// responsibility (ADR-0037's own directed-dial ordering),
        /// mirrored symmetrically at every peer.
        dial_target: Option<SocketAddr>,
    },
    PeerMessage {
        conn_id: u64,
        envelope: P2PMessageEnvelopeV1,
    },
    /// A connection's reader thread exited — the peer disconnected, or
    /// sent malformed bytes (ADR-0039).
    ConnectionClosed {
        conn_id: u64,
    },
    ProposeTimeout {
        height: u64,
        round: u64,
    },
    PrevoteTimeout {
        height: u64,
        round: u64,
    },
    PrecommitTimeout {
        height: u64,
        round: u64,
    },
    StartupGraceElapsed,
}

struct Ctx {
    engine: ConsensusEngine,
    store: RedbStateStore,
    records: Vec<ValidatorRecordV1>,
    ordered_ids: Vec<Digest>,
    own_validator_id: Digest,
    consensus_keypair: Ed25519KeyPair,
    network_key_seed: [u8; 32],
    chain_id: u8,
    network_id: u16,
    base_timeout_ms: u64,
    handshake_params: HandshakeParams,
    event_tx: Sender<NodeEvent>,
    raw_tx: Sender<(u64, P2PMessageEnvelopeV1)>,
    closed_tx: Sender<u64>,
    conn_id_counter: Arc<AtomicU64>,
    links: HashMap<u64, PeerLink>,
    handshakes: HashMap<u64, HandshakeState>,
    established: HashSet<u64>,
    /// `conn_id -> the address this node dialed to establish it`, for
    /// connections this node itself initiated only (ADR-0039, "Decided:
    /// Connection-Drop Reconnection") — consulted on
    /// `NodeEvent::ConnectionClosed` to decide whether to redial.
    dialed_peers: HashMap<u64, SocketAddr>,
    round_started: bool,
}

/// Runs this validator's node process until killed (ADR-0038,
/// "Decided: Node Config"/"Decided: DB Init"/"Decided: Own Validator
/// Identity Resolution"). Loads and validates a real genesis file,
/// opens (or resumes, with an integrity check) a durable
/// `RedbStateStore`, connects to every configured peer (directed dial
/// by listen-address ordering) or gives up waiting after a bounded
/// startup grace period, then drives
/// [`hn_consensus::ConsensusEngine`] entirely off real wall-clock
/// timers and real network messages.
pub fn run(config: NodeConfig) -> NodeResult<()> {
    let manifest = GenesisManifest::load(&config.genesis_path)?;

    std::fs::create_dir_all(&config.data_dir)?;
    let mut store = RedbStateStore::open(config.data_dir.join("state.redb"))?;
    init_genesis(&mut store, &manifest)?;

    let records: Vec<ValidatorRecordV1> = manifest
        .validators
        .iter()
        .map(|validator| ValidatorRecordV1 {
            validator_id: validator.validator_id,
            consensus_key: validator.consensus_key,
            bonded_stake: validator.bonded_stake,
            voting_power: validator.bonded_stake,
            status: ValidatorStatus::Active,
            pending_unbonding: None,
        })
        .collect();
    let ordered_active_set = active_set(&records, records.len());
    let ordered_ids: Vec<Digest> = ordered_active_set
        .iter()
        .map(|record| record.validator_id)
        .collect();

    let consensus_keypair =
        keypair_from_seed(KeyRole::ValidatorConsensus, config.consensus_key_seed);
    let own_validator_id = resolve_own_validator_id(&consensus_keypair, &manifest.validators)
        .ok_or_else(|| {
            NodeError::from(crate::config::ConfigError(
                "configured --consensus-key-seed does not match any genesis validator".to_string(),
            ))
        })?;

    let handshake_params = HandshakeParams {
        protocol_version: PROTOCOL_VERSION,
        chain_id: manifest.chain_id,
        network_id: manifest.network_id,
        required_channels: vec![Channel::Handshake, Channel::Consensus],
        required_message_types: vec![
            MessageType::Hello,
            MessageType::ConsensusProposal,
            MessageType::ConsensusVote,
            MessageType::QuorumCertificateMessage,
        ],
    };

    let (event_tx, event_rx) = mpsc::channel::<NodeEvent>();
    let (raw_tx, raw_rx) = mpsc::channel::<(u64, P2PMessageEnvelopeV1)>();
    let (closed_tx, closed_rx) = mpsc::channel::<u64>();
    {
        let event_tx = event_tx.clone();
        thread::spawn(move || {
            for (conn_id, envelope) in raw_rx {
                if event_tx
                    .send(NodeEvent::PeerMessage { conn_id, envelope })
                    .is_err()
                {
                    break;
                }
            }
        });
    }
    {
        let event_tx = event_tx.clone();
        thread::spawn(move || {
            for conn_id in closed_rx {
                if event_tx
                    .send(NodeEvent::ConnectionClosed { conn_id })
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    let conn_id_counter = Arc::new(AtomicU64::new(0));
    let listener = std::net::TcpListener::bind(config.listen)?;
    spawn_listener(
        listener,
        raw_tx.clone(),
        closed_tx.clone(),
        event_tx.clone(),
        Arc::clone(&conn_id_counter),
    );

    for &peer in &config.peers {
        if config.listen < peer {
            spawn_dialer(
                peer,
                raw_tx.clone(),
                closed_tx.clone(),
                event_tx.clone(),
                Arc::clone(&conn_id_counter),
            );
        }
    }

    spawn_after(
        event_tx.clone(),
        Duration::from_millis(config.base_timeout_ms.saturating_mul(4)),
        NodeEvent::StartupGraceElapsed,
    );

    let mut ctx = Ctx {
        engine: ConsensusEngine::new_height(BlockHeight::GENESIS),
        store,
        records,
        ordered_ids,
        own_validator_id,
        consensus_keypair,
        network_key_seed: config.network_key_seed,
        chain_id: manifest.chain_id,
        network_id: manifest.network_id,
        base_timeout_ms: config.base_timeout_ms,
        handshake_params,
        event_tx,
        raw_tx,
        closed_tx,
        conn_id_counter,
        links: HashMap::new(),
        handshakes: HashMap::new(),
        established: HashSet::new(),
        dialed_peers: HashMap::new(),
        round_started: false,
    };

    drive(&mut ctx, event_rx, config.peers.len())
}

/// If no genesis marker is stored yet, validates `manifest` was already
/// (`GenesisManifest::load`), applies its write-set, and stores the
/// marker. If a marker already exists, recomputes `manifest`'s own
/// `genesis_hash` and rejects a mismatch (ADR-0038, "Decided: DB
/// Init").
fn init_genesis(store: &mut RedbStateStore, manifest: &GenesisManifest) -> NodeResult<()> {
    let marker_key = genesis_marker_key()?;
    let genesis_hash = manifest.genesis_hash()?;

    match store.get(&marker_key)? {
        None => {
            let mut writes = manifest.genesis_write_set()?;
            writes.push(Write {
                state_key: marker_key,
                value: genesis_hash.to_vec(),
            });
            store.commit(&writes)?;
            Ok(())
        }
        Some(stored) if stored == genesis_hash.to_vec() => Ok(()),
        Some(_) => Err(NodeError::from(crate::config::ConfigError(
            "--data-dir was initialized from a different genesis file (genesis_hash mismatch)"
                .to_string(),
        ))),
    }
}

fn spawn_listener(
    listener: std::net::TcpListener,
    raw_tx: Sender<(u64, P2PMessageEnvelopeV1)>,
    closed_tx: Sender<u64>,
    event_tx: Sender<NodeEvent>,
    counter: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        for accepted in listener.incoming() {
            let Ok(stream) = accepted else { continue };
            let conn_id = counter.fetch_add(1, Ordering::Relaxed);
            let Ok(link) = spawn_peer_link(stream, conn_id, raw_tx.clone(), closed_tx.clone())
            else {
                continue;
            };
            if event_tx
                .send(NodeEvent::NewConnection {
                    conn_id,
                    link,
                    dial_target: None,
                })
                .is_err()
            {
                break;
            }
        }
    });
}

/// Dials `addr` once (retrying every 100ms until it succeeds), reports
/// the resulting connection, then exits. Reconnection after a
/// *later* drop is not this thread's own job — the driving loop calls
/// this function again from scratch on `NodeEvent::ConnectionClosed`
/// (ADR-0039, "Decided: Connection-Drop Reconnection"), rather than
/// this thread looping forever and monitoring the link itself.
fn spawn_dialer(
    addr: SocketAddr,
    raw_tx: Sender<(u64, P2PMessageEnvelopeV1)>,
    closed_tx: Sender<u64>,
    event_tx: Sender<NodeEvent>,
    counter: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        loop {
            match TcpStream::connect(addr) {
                Ok(stream) => {
                    let conn_id = counter.fetch_add(1, Ordering::Relaxed);
                    let Ok(link) = spawn_peer_link(stream, conn_id, raw_tx, closed_tx) else {
                        return;
                    };
                    let _ = event_tx.send(NodeEvent::NewConnection {
                        conn_id,
                        link,
                        dial_target: Some(addr),
                    });
                    return;
                }
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
    });
}

fn spawn_after(event_tx: Sender<NodeEvent>, delay: Duration, event: NodeEvent) {
    thread::spawn(move || {
        thread::sleep(delay);
        let _ = event_tx.send(event);
    });
}

fn stage_timeout(base_timeout_ms: u64, round: Round) -> Duration {
    Duration::from_millis(base_timeout_ms.saturating_mul(round.get().saturating_add(1)))
}

fn drive(ctx: &mut Ctx, event_rx: Receiver<NodeEvent>, required_peers: usize) -> NodeResult<()> {
    for event in event_rx {
        match event {
            NodeEvent::NewConnection {
                conn_id,
                link,
                dial_target,
            } => {
                ctx.links.insert(conn_id, link);
                if let Some(addr) = dial_target {
                    ctx.dialed_peers.insert(conn_id, addr);
                }
                let mut handshake = HandshakeState::new(
                    ctx.handshake_params.clone(),
                    keypair_from_seed(KeyRole::ValidatorNetwork, ctx.network_key_seed),
                );
                if let Ok(HandshakeAction::Send(hello)) = handshake.apply(HandshakeEvent::SendHello)
                {
                    send_hello(ctx, conn_id, &hello)?;
                }
                ctx.handshakes.insert(conn_id, handshake);
            }
            NodeEvent::PeerMessage { conn_id, envelope } => {
                handle_peer_message(ctx, conn_id, envelope)?;
                if !ctx.round_started && ctx.established.len() >= required_peers {
                    ctx.round_started = true;
                    let action = ctx.engine.begin_round()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::ConnectionClosed { conn_id } => {
                ctx.links.remove(&conn_id);
                ctx.handshakes.remove(&conn_id);
                ctx.established.remove(&conn_id);
                if let Some(addr) = ctx.dialed_peers.remove(&conn_id) {
                    // This node was responsible for this link (it
                    // dialed `addr`) -- redial, exactly as at startup,
                    // so this node keeps trying to reach `addr` again
                    // once it comes back (ADR-0039, "Decided:
                    // Connection-Drop Reconnection"). A dropped
                    // *inbound* connection needs no action here: the
                    // peer that dialed *us* owns its own reconnection.
                    spawn_dialer(
                        addr,
                        ctx.raw_tx.clone(),
                        ctx.closed_tx.clone(),
                        ctx.event_tx.clone(),
                        Arc::clone(&ctx.conn_id_counter),
                    );
                }
            }
            NodeEvent::ProposeTimeout { height, round } => {
                if is_current(ctx, height, round)
                    && ctx.engine.state().step == hn_consensus::ConsensusStep::Propose
                {
                    let action = ctx.engine.propose_timeout()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::PrevoteTimeout { height, round } => {
                if is_current(ctx, height, round)
                    && ctx.engine.state().step == hn_consensus::ConsensusStep::Prevote
                {
                    let action = ctx.engine.prevote_timeout()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::PrecommitTimeout { height, round } => {
                if is_current(ctx, height, round)
                    && ctx.engine.state().step == hn_consensus::ConsensusStep::Precommit
                {
                    let action = ctx.engine.precommit_timeout()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::StartupGraceElapsed => {
                if !ctx.round_started {
                    ctx.round_started = true;
                    let action = ctx.engine.begin_round()?;
                    act_on(ctx, action)?;
                }
            }
        }
    }
    Ok(())
}

fn is_current(ctx: &Ctx, height: u64, round: u64) -> bool {
    ctx.engine.state().height.get() == height && ctx.engine.state().round.get() == round
}

// Every timeout branch above checks *both* `is_current` (height/round)
// *and* the specific step the timeout is for -- (height, round) alone
// is not enough: entering `Propose` always schedules a
// `ProposeTimeout`, but a node that is also this round's proposer
// self-proposes and casts a `Prevote` synchronously, in the same call,
// before that timer ever fires -- leaving a stale `ProposeTimeout` for
// the *same* (height, round) pending while the engine has already
// moved on to `Prevote`. Found by running a genuinely slow round (a
// single node with no peers, which can never reach quorum and so never
// advances height/round quickly): `is_current` alone let the stale
// timer through and `ConsensusEngine::propose_timeout` correctly
// rejected it with `UnexpectedEvent { step: Prevote }` -- a real,
// closed-state-machine safety net catching a genuine driving-loop bug,
// not a false alarm. Multi-node happy-path runs hid this because a
// real quorum typically arrives well within one timeout window, so by
// the time a stale timer fires the round has already advanced past it
// and the weaker `is_current`-only check already rejected it, just for
// the wrong reason.

fn send_hello(ctx: &Ctx, conn_id: u64, hello: &Hello) -> NodeResult<()> {
    let Some(link) = ctx.links.get(&conn_id) else {
        return Ok(());
    };
    let envelope = P2PMessageEnvelopeV1::new(
        PROTOCOL_VERSION,
        ctx.chain_id,
        ctx.network_id,
        Channel::Handshake,
        MessageType::Hello,
        hello.encode()?,
    )?;
    let _ = link.send(envelope);
    Ok(())
}

fn handle_peer_message(
    ctx: &mut Ctx,
    conn_id: u64,
    envelope: P2PMessageEnvelopeV1,
) -> NodeResult<()> {
    if envelope.chain_id != ctx.chain_id || envelope.network_id != ctx.network_id {
        return Ok(());
    }

    if let Some(mut handshake) = ctx.handshakes.remove(&conn_id) {
        if envelope.channel != Channel::Handshake || envelope.message_type != MessageType::Hello {
            // Not yet established; anything else is untrusted -- drop.
            ctx.handshakes.insert(conn_id, handshake);
            return Ok(());
        }
        let hello = Hello::decode(&envelope.payload)?;
        match handshake.apply(HandshakeEvent::ReceiveHello(hello))? {
            HandshakeAction::Accepted { .. } => {
                // Admission control only -- see ADR-0038, "Decided:
                // Connection-Layer Simplification": consensus messages
                // authenticate themselves independently of which
                // connection delivered them, so this connection layer
                // does not need (and no longer tries) to resolve which
                // validator is on the other end.
                ctx.established.insert(conn_id);
            }
            HandshakeAction::Rejected(_) | HandshakeAction::Send(_) => {}
        }
        return Ok(());
    }
    if !ctx.established.contains(&conn_id) {
        return Ok(());
    }

    match (envelope.channel, envelope.message_type) {
        (Channel::Consensus, MessageType::ConsensusVote) => {
            let vote = ConsensusVote::decode(&envelope.payload)?;
            maybe_sync_forward(ctx, vote.payload.height.get())?;
            if let Some(qc) = ctx.engine.record_vote(&vote, &ctx.store, &ctx.records)? {
                broadcast(
                    ctx,
                    Channel::Consensus,
                    MessageType::QuorumCertificateMessage,
                    qc.encode()?,
                )?;
                if expects_qc(ctx, &qc) {
                    let action =
                        ctx.engine
                            .handle_quorum_certificate(qc, &ctx.store, &ctx.ordered_ids)?;
                    act_on(ctx, action)?;
                }
            }
        }
        (Channel::Consensus, MessageType::QuorumCertificateMessage) => {
            let qc = QuorumCertificate::decode(&envelope.payload)?;
            maybe_sync_forward(ctx, qc.height.get())?;
            if expects_qc(ctx, &qc) {
                let action =
                    ctx.engine
                        .handle_quorum_certificate(qc, &ctx.store, &ctx.ordered_ids)?;
                act_on(ctx, action)?;
            }
        }
        (Channel::Consensus, MessageType::ConsensusProposal) => {
            let message = ConsensusProposalMessageV1::decode(&envelope.payload)?;
            maybe_sync_forward(ctx, message.height.get())?;
            // A proposal that does not target this engine's own exact
            // current (height, round, step) is either stale (a round
            // already left behind) or ahead of what
            // `maybe_sync_forward` just caught up to (round > 0 at the
            // new height) -- either way it has no defined transition
            // from wherever the engine actually is right now, so it is
            // silently dropped rather than fed to
            // `ConsensusEngine::handle_proposal` (ADR-0034's own closed,
            // total `apply` would reject it as `UnexpectedEvent`).
            // `ConsensusProposalMessageV1.height`/`.round` (ADR-0039)
            // make this an exact check now, not the weaker
            // step-only guard this branch used before.
            if message.height.get() == ctx.engine.state().height.get()
                && message.round.get() == ctx.engine.state().round.get()
                && ctx.engine.state().step == hn_consensus::ConsensusStep::Propose
            {
                let action = ctx.engine.handle_proposal(
                    message.block_hash,
                    message.transactions,
                    message.justification,
                )?;
                act_on(ctx, action)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// If `observed_height` is strictly ahead of this engine's own current
/// height, this node has fallen behind the network's actual progress —
/// most realistically after restarting while its peers kept finalizing
/// without it (ADR-0039, "Decided: Passive Height-Observation
/// Catch-Up"). No dedicated sync request/response message exists (or
/// is needed): `ConsensusVote`/`QuorumCertificate` already self-report
/// their own `height`, and a rejoining node starts observing them as
/// soon as any connection's handshake completes and the network's
/// already-constant vote gossip reaches it. Jumps straight to
/// `observed_height` at round 0 (no lock — exactly what
/// `ConsensusEngine::new_height` already gives a fresh height) rather
/// than replaying every intermediate height's content: this pass's own
/// blocks are always empty (proving liveness/view-change/restart-
/// recovery is the goal, not state transitions), so there is no actual
/// application state to reconstruct height-by-height — a real future
/// pass with real transactions would need real block/state sync
/// (ADR-0016's own still-entirely-open territory), not this shortcut.
/// If the jump lands mid-round relative to what peers have actually
/// reached (they are past round 0 by the time this node catches up),
/// this node's own round-0 attempt simply times out through the normal
/// propose/prevote/precommit cycle like any other failed round,
/// catching up the rest of the way a round at a time — slower than
/// jumping straight to the right round, but reuses every existing
/// mechanism with no new one needed.
///
/// Never triggered by an already-caught-up or genuinely stale (at or
/// behind current) observation — `observed_height` must be strictly
/// greater, so every jump this function performs is monotonically
/// forward.
fn maybe_sync_forward(ctx: &mut Ctx, observed_height: u64) -> NodeResult<()> {
    if observed_height <= ctx.engine.state().height.get() {
        return Ok(());
    }
    log_line(&format!(
        "SYNCED to height={observed_height} (observed from a peer)"
    ));
    ctx.engine = ConsensusEngine::new_height(BlockHeight::new(observed_height));
    ctx.round_started = true;
    let action = ctx.engine.begin_round()?;
    act_on(ctx, action)
}

/// Whether `qc` still has a defined transition from the engine's
/// *current* step. Two independent, honest nodes each aggregating
/// their own [`QuorumCertificate`] from the same gossiped votes (see
/// ADR-0037, "Decided: Vote Aggregation" — no designated aggregator)
/// routinely produces more than one certificate for the same round: a
/// node that already consumed its own self-built prevote quorum and
/// moved on to `Precommit` will still receive the peer's broadcast
/// prevote quorum a moment later. `hn_consensus::ConsensusState::apply`
/// is deliberately a closed, total function that rejects rather than
/// silently ignores an event with no transition (ADR-0034's own
/// design), so this driving loop must filter out an already-redundant
/// certificate itself before ever calling
/// `ConsensusEngine::handle_quorum_certificate` — the same "stale
/// event, not an error" handling already applied to timeout events.
fn expects_qc(ctx: &Ctx, qc: &QuorumCertificate) -> bool {
    if !is_current(ctx, qc.height.get(), qc.round.get()) {
        return false;
    }
    match qc.certificate_type {
        VoteType::Prevote => ctx.engine.state().step == hn_consensus::ConsensusStep::Prevote,
        VoteType::Precommit => ctx.engine.state().step == hn_consensus::ConsensusStep::Precommit,
    }
}

fn broadcast(
    ctx: &Ctx,
    channel: Channel,
    message_type: MessageType,
    payload: Vec<u8>,
) -> NodeResult<()> {
    let envelope = P2PMessageEnvelopeV1::new(
        PROTOCOL_VERSION,
        ctx.chain_id,
        ctx.network_id,
        channel,
        message_type,
        payload,
    )?;
    for conn_id in &ctx.established {
        if let Some(link) = ctx.links.get(conn_id) {
            let _ = link.send(envelope.clone());
        }
    }
    Ok(())
}

fn act_on(ctx: &mut Ctx, action: ConsensusAction) -> NodeResult<()> {
    match action {
        ConsensusAction::None => maybe_start_propose_stage(ctx)?,
        ConsensusAction::Prevote(target) => cast_vote(ctx, VoteType::Prevote, target)?,
        ConsensusAction::Precommit(target) => cast_vote(ctx, VoteType::Precommit, target)?,
        ConsensusAction::Finalized { block_hash, .. } => {
            let height = ctx.engine.state().height;
            let round = ctx.engine.state().round;
            let result =
                ctx.engine
                    .commit_finalized_block(block_hash, &mut ctx.store, &ctx.records)?;
            log_line(&format!(
                "FINALIZED height={} round={} block={} applied={}",
                height.get(),
                round.get(),
                hex(&block_hash),
                result.applied.len()
            ));
            let action = ctx.engine.begin_new_height()?;
            act_on(ctx, action)?;
        }
        ConsensusAction::RoundAdvanced { .. } | ConsensusAction::NewHeight { .. } => {
            let action = ctx.engine.begin_round()?;
            act_on(ctx, action)?;
        }
    }
    Ok(())
}

fn maybe_start_propose_stage(ctx: &mut Ctx) -> NodeResult<()> {
    let height = ctx.engine.state().height;
    let round = ctx.engine.state().round;
    spawn_after(
        ctx.event_tx.clone(),
        stage_timeout(ctx.base_timeout_ms, round),
        NodeEvent::ProposeTimeout {
            height: height.get(),
            round: round.get(),
        },
    );

    if round_proposer(&ctx.ordered_ids, height, round) == Some(ctx.own_validator_id) {
        let block_hash = synthetic_block_hash(height, round, ctx.own_validator_id);
        broadcast(
            ctx,
            Channel::Consensus,
            MessageType::ConsensusProposal,
            ConsensusProposalMessageV1 {
                height,
                round,
                block_hash,
                transactions: Vec::new(),
                justification: None,
            }
            .encode()?,
        )?;
        let action = ctx.engine.handle_proposal(block_hash, Vec::new(), None)?;
        act_on(ctx, action)?;
    }
    Ok(())
}

fn cast_vote(ctx: &mut Ctx, vote_type: VoteType, target: ConsensusTarget) -> NodeResult<()> {
    let (target_type, target_hash) = match target {
        ConsensusTarget::Block(hash) => (VoteTargetType::Block, hash),
        ConsensusTarget::Nil => (VoteTargetType::Nil, [0_u8; 32]),
    };
    let height = ctx.engine.state().height;
    let round = ctx.engine.state().round;

    let payload = VoteSigningPayloadV1 {
        vote_type,
        chain_id: ctx.chain_id,
        network_id: ctx.network_id,
        epoch: Epoch::new(0),
        height,
        round,
        validator_set_commitment: DEVNET_VSC,
        validator_id: ctx.own_validator_id,
        target_type,
        target_hash,
        vote_metadata: Vec::new(),
    };
    let digest = payload.signing_digest()?;
    let signature = SignatureEnvelope {
        algorithm_id: ctx.consensus_keypair.key_descriptor().algorithm_id(),
        key_reference: None,
        signature: ctx.consensus_keypair.sign(&digest).to_vec(),
    };
    let vote = ConsensusVote { payload, signature };

    broadcast(
        ctx,
        Channel::Consensus,
        MessageType::ConsensusVote,
        vote.encode()?,
    )?;

    let stage_round = round;
    let stage_height = height;
    spawn_after(
        ctx.event_tx.clone(),
        stage_timeout(ctx.base_timeout_ms, stage_round),
        match vote_type {
            VoteType::Prevote => NodeEvent::PrevoteTimeout {
                height: stage_height.get(),
                round: stage_round.get(),
            },
            VoteType::Precommit => NodeEvent::PrecommitTimeout {
                height: stage_height.get(),
                round: stage_round.get(),
            },
        },
    );

    if let Some(qc) = ctx.engine.record_vote(&vote, &ctx.store, &ctx.records)? {
        broadcast(
            ctx,
            Channel::Consensus,
            MessageType::QuorumCertificateMessage,
            qc.encode()?,
        )?;
        if expects_qc(ctx, &qc) {
            let action = ctx
                .engine
                .handle_quorum_certificate(qc, &ctx.store, &ctx.ordered_ids)?;
            act_on(ctx, action)?;
        }
    }
    Ok(())
}

fn synthetic_block_hash(height: BlockHeight, round: Round, proposer: Digest) -> Digest {
    // Not ADR-0008's real block hash (which hashes a block's actual
    // transactions/header) -- this pass's blocks always carry zero
    // transactions (proving liveness/view change is the goal, not state
    // transitions), so a per-round-unique opaque identifier is all a
    // real proposal object needs to exist. Plain concatenation, not a
    // domain-separated hash: this identifier carries no cryptographic
    // meaning to verify, so it does not belong in ADR-0005's registry.
    let mut bytes = [0_u8; 32];
    bytes[0..8].copy_from_slice(&height.get().to_le_bytes());
    bytes[8..16].copy_from_slice(&round.get().to_le_bytes());
    bytes[16..32].copy_from_slice(&proposer[0..16]);
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Prints `line` to stdout and flushes immediately. `std::io::Stdout`
/// is not guaranteed to reach a redirected file/pipe promptly on its
/// own, and this crate deliberately installs no shutdown signal handler
/// (ADR-0038, "Decided: No Custom Signal Handling") — without an
/// explicit flush, a process killed shortly after logging a line (a
/// real, expected event for a "stop" this crate treats as ordinary OS
/// termination) could lose that line entirely, undermining the only
/// operational visibility this daemon has.
fn log_line(line: &str) {
    println!("{line}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}
