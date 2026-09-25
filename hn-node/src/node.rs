use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use hn_consensus::{ConsensusAction, ConsensusEngine, ConsensusTarget, round_proposer};
use hn_core::{BlockHeight, Epoch, ProtocolVersion, Round};
use hn_crypto::{Digest, Ed25519KeyPair, KeyDescriptor, SignatureEnvelope};
use hn_network::{
    Channel, ConsensusProposalMessageV1, HandshakeAction, HandshakeEvent, HandshakeParams,
    HandshakeState, Hello, MessageType, P2PMessageEnvelopeV1, PeerLink, spawn_peer_link,
};
use hn_state::{
    ConsensusVote, QuorumCertificate, ValidatorRecordV1, VoteSigningPayloadV1, VoteTargetType,
    VoteType, active_set,
};
use hn_storage::RedbStateStore;

use crate::config::NodeConfig;
use crate::error::{NodeError, NodeResult};
use crate::genesis::{devnet_validator_records, ensure_genesis_written};
use crate::identity::{consensus_keypair, network_keypair, peer_addr, validator_id};

/// A fixed devnet `validator_set_commitment` (ADR-0037's own devnet
/// scope never defines `ValidatorSetCommitmentV1`'s real canonical
/// bytes — matches the established test convention throughout this
/// codebase of a fixed placeholder constant; nothing here cross-checks
/// it against a computed value).
const DEVNET_VSC: [u8; 32] = [0x11; 32];

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(0, 1, 0);

/// What the driving loop reacts to (ADR-0037, "Decided: Round Timer /
/// Driving Loop").
enum NodeEvent {
    NewConnection {
        conn_id: u64,
        link: PeerLink,
    },
    PeerMessage {
        conn_id: u64,
        envelope: P2PMessageEnvelopeV1,
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
    own_index: u8,
    own_validator_id: [u8; 32],
    consensus_keypair: Ed25519KeyPair,
    chain_id: u8,
    network_id: u16,
    base_timeout_ms: u64,
    handshake_params: HandshakeParams,
    event_tx: Sender<NodeEvent>,
    links: HashMap<u64, PeerLink>,
    handshakes: HashMap<u64, HandshakeState>,
    conn_by_validator: HashMap<u8, u64>,
    round_started: bool,
}

/// Runs this validator's node process until killed (ADR-0037, "Decided:
/// `hn-node` Process"). Opens (or resumes) a real, durable
/// `RedbStateStore`, writes every devnet validator's record if not
/// already present, connects to every configured peer (directed dial,
/// ADR-0037's own "Decided: Connection Topology") or gives up waiting
/// after a bounded startup grace period, then drives
/// [`hn_consensus::ConsensusEngine`] entirely off real wall-clock
/// timers and real network messages — no in-process shortcuts.
pub fn run(config: NodeConfig) -> NodeResult<()> {
    if config.validator_index >= config.validator_count {
        return Err(NodeError::from(crate::config::ConfigError(
            "validator-index must be less than validator-count".to_string(),
        )));
    }

    std::fs::create_dir_all(&config.data_dir)?;
    let mut store = RedbStateStore::open(config.data_dir.join("state.redb"))?;

    let records = devnet_validator_records(config.validator_count);
    ensure_genesis_written(&mut store, &records)?;
    let ordered_active_set = active_set(&records, records.len());
    let ordered_ids: Vec<Digest> = ordered_active_set
        .iter()
        .map(|record| record.validator_id)
        .collect();

    let handshake_params = HandshakeParams {
        protocol_version: PROTOCOL_VERSION,
        chain_id: config.chain_id,
        network_id: config.network_id,
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

    let conn_id_counter = Arc::new(AtomicU64::new(0));
    let own_addr = peer_addr(config.base_port, config.validator_index);
    let listener = std::net::TcpListener::bind(own_addr)?;
    spawn_listener(
        listener,
        raw_tx.clone(),
        event_tx.clone(),
        Arc::clone(&conn_id_counter),
    );

    for peer_index in (config.validator_index + 1)..config.validator_count {
        spawn_dialer(
            peer_addr(config.base_port, peer_index),
            raw_tx.clone(),
            event_tx.clone(),
            Arc::clone(&conn_id_counter),
        );
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
        own_index: config.validator_index,
        own_validator_id: validator_id(config.validator_index),
        consensus_keypair: consensus_keypair(config.validator_index),
        chain_id: config.chain_id,
        network_id: config.network_id,
        base_timeout_ms: config.base_timeout_ms,
        handshake_params,
        event_tx,
        links: HashMap::new(),
        handshakes: HashMap::new(),
        conn_by_validator: HashMap::new(),
        round_started: false,
    };

    let required_peers = usize::from(config.validator_count.saturating_sub(1));
    drive(&mut ctx, event_rx, required_peers)
}

fn spawn_listener(
    listener: std::net::TcpListener,
    raw_tx: Sender<(u64, P2PMessageEnvelopeV1)>,
    event_tx: Sender<NodeEvent>,
    counter: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        for accepted in listener.incoming() {
            let Ok(stream) = accepted else { continue };
            let conn_id = counter.fetch_add(1, Ordering::Relaxed);
            let Ok(link) = spawn_peer_link(stream, conn_id, raw_tx.clone()) else {
                continue;
            };
            if event_tx
                .send(NodeEvent::NewConnection { conn_id, link })
                .is_err()
            {
                break;
            }
        }
    });
}

fn spawn_dialer(
    addr: std::net::SocketAddr,
    raw_tx: Sender<(u64, P2PMessageEnvelopeV1)>,
    event_tx: Sender<NodeEvent>,
    counter: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        loop {
            match TcpStream::connect(addr) {
                Ok(stream) => {
                    let conn_id = counter.fetch_add(1, Ordering::Relaxed);
                    let Ok(link) = spawn_peer_link(stream, conn_id, raw_tx) else {
                        return;
                    };
                    let _ = event_tx.send(NodeEvent::NewConnection { conn_id, link });
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
            NodeEvent::NewConnection { conn_id, link } => {
                ctx.links.insert(conn_id, link);
                let mut handshake = HandshakeState::new(
                    ctx.handshake_params.clone(),
                    network_keypair(ctx.own_index),
                );
                if let Ok(HandshakeAction::Send(hello)) = handshake.apply(HandshakeEvent::SendHello)
                {
                    send_hello(ctx, conn_id, &hello)?;
                }
                ctx.handshakes.insert(conn_id, handshake);
            }
            NodeEvent::PeerMessage { conn_id, envelope } => {
                handle_peer_message(ctx, conn_id, envelope)?;
                if !ctx.round_started && ctx.conn_by_validator.len() >= required_peers {
                    ctx.round_started = true;
                    let action = ctx.engine.begin_round()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::ProposeTimeout { height, round } => {
                if is_current(ctx, height, round) {
                    let action = ctx.engine.propose_timeout()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::PrevoteTimeout { height, round } => {
                if is_current(ctx, height, round) {
                    let action = ctx.engine.prevote_timeout()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::PrecommitTimeout { height, round } => {
                if is_current(ctx, height, round) {
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

fn resolve_validator_index(ctx: &Ctx, node_key: &KeyDescriptor) -> Option<u8> {
    let bytes = node_key.public_key_bytes();
    (0..ctx.records.len() as u8)
        .find(|&index| network_keypair(index).key_descriptor().public_key_bytes() == bytes)
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
            HandshakeAction::Accepted { peer_hello, .. } => {
                if let Some(peer_index) = resolve_validator_index(ctx, &peer_hello.node_key) {
                    ctx.conn_by_validator.insert(peer_index, conn_id);
                }
            }
            HandshakeAction::Rejected(_) | HandshakeAction::Send(_) => {}
        }
        return Ok(());
    }

    match (envelope.channel, envelope.message_type) {
        (Channel::Consensus, MessageType::ConsensusVote) => {
            let vote = ConsensusVote::decode(&envelope.payload)?;
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
            if expects_qc(ctx, &qc) {
                let action =
                    ctx.engine
                        .handle_quorum_certificate(qc, &ctx.store, &ctx.ordered_ids)?;
                act_on(ctx, action)?;
            }
        }
        // A late proposal for a round already left behind (this node
        // moved on to `Prevote`/`Precommit`/beyond by the time it
        // arrived) has no defined transition from the current step --
        // the same "stale event" filtering `expects_qc` applies to
        // certificates, needed here too since `ConsensusEvent::Proposal`
        // carries no height/round of its own to check against (it
        // always targets whatever round `ConsensusState` is currently
        // attempting).
        (Channel::Consensus, MessageType::ConsensusProposal)
            if ctx.engine.state().step == hn_consensus::ConsensusStep::Propose =>
        {
            let message = ConsensusProposalMessageV1::decode(&envelope.payload)?;
            let action = ctx.engine.handle_proposal(
                message.block_hash,
                message.transactions,
                message.justification,
            )?;
            act_on(ctx, action)?;
        }
        _ => {}
    }
    Ok(())
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
    for conn_id in ctx.conn_by_validator.values() {
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
            println!(
                "FINALIZED height={} round={} block={} applied={}",
                height.get(),
                round.get(),
                hex(&block_hash),
                result.applied.len()
            );
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

fn synthetic_block_hash(height: BlockHeight, round: Round, proposer: [u8; 32]) -> [u8; 32] {
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
