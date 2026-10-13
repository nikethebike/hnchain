use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use hn_consensus::{
    ConsensusAction, ConsensusEngine, ConsensusError, ConsensusTarget, round_proposer,
};
use hn_core::{BlockHeight, Epoch, ProtocolEpoch, ProtocolVersion, Round, UnixTimeMillis};
use hn_crypto::{Digest, Ed25519KeyPair, KeyRole, SignatureEnvelope, hash_profile_0x0001};
use hn_network::{
    Channel, ConsensusProposalMessageV1, HandshakeAction, HandshakeEvent, HandshakeParams,
    HandshakeState, Hello, MAX_SYNC_BLOCKS_PER_RESPONSE, MessageType, P2PMessageEnvelopeV1,
    PeerLink, SyncBlockRangeRequestV1, SyncBlockRangeResponseV1, TransactionAnnounceV1,
    TransactionRequestV1, TransactionResponseV1, spawn_peer_link,
};
use hn_state::{
    BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader, BlockStore,
    ConsensusVote, EmptyHashTable, FinalizedBlock, HEADER_VERSION_1, QuorumCertificate, ReceiptV1,
    StateCommitter, StateEnumerator, StateReader, ValidatorRecordV1, ValidatorStatus,
    VoteSigningPayloadV1, VoteTargetType, VoteType, Write, active_key, active_set, apply_block,
    compute_post_state_root, list_empty_root, protocol_parameters_placeholder_hash,
};
use hn_storage::RedbStateStore;

use crate::config::NodeConfig;
use crate::error::{NodeError, NodeResult};
use crate::genesis::GenesisManifest;
use crate::identity::{keypair_from_seed, resolve_own_validator_id};
use crate::mempool::Mempool;

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

/// A [`StateEnumerator`] that hides [`genesis_marker_key`] from whatever
/// it wraps. That marker is stored through the same
/// [`StateCommitter`]/table as real consensus state (ADR-0038, "Decided:
/// DB Init" predates `StateEnumerator` and never anticipated a full-table
/// scan), so a bare enumeration of a node's own store is not the real
/// consensus state leaf set and must never be hashed into a `state_root`
/// directly — `compute_post_state_root` needs this filtered view instead.
struct ExcludingGenesisMarker<'a, E> {
    inner: &'a E,
    excluded: Digest,
}

impl<E: StateEnumerator> StateEnumerator for ExcludingGenesisMarker<'_, E> {
    fn enumerate(&self) -> hn_state::StateResult<Vec<(Digest, Vec<u8>)>> {
        Ok(self
            .inner
            .enumerate()?
            .into_iter()
            .filter(|(key, _)| *key != self.excluded)
            .collect())
    }
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
    /// No `SyncBlockRangeResponse` arrived in time for the request
    /// `request_next_sync_range` sent to `peer_conn_id` starting at
    /// `from_height` (ADR-0044's own addendum on request timeouts).
    SyncRequestTimeout {
        peer_conn_id: u64,
        from_height: u64,
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
    /// The previous block's own real `block_hash`, chained forward
    /// every time a block finalizes — `[0; 32]` before the first block
    /// (genesis's own `parent_block_hash` semantics remain an open
    /// decision, ADR-0008; this sentinel is this devnet's own
    /// unambiguous "no real parent yet" value, not a claim about what
    /// a real genesis block's `parent_block_hash` should be).
    last_block_hash: Digest,
    /// The previous block's own real `header.timestamp`, chained
    /// forward the same way as `last_block_hash` — `UnixTimeMillis::
    /// from_millis(0)` before the first block, the same class of
    /// sentinel. Used by [`verify_proposal`]'s timestamp-monotonicity
    /// check (ADR-0042). Always correct now (ADR-0044, "Decided:
    /// Replacing The Passive Jump For Real Catch-Up"): every height
    /// this node ever reaches is either locally finalized while live,
    /// or reached via [`verify_and_commit_synced_block`]'s own full,
    /// verified replay, which maintains this field block by block —
    /// there is no longer a passive jump that could leave it stale, so
    /// (unlike before ADR-0044) nothing ever needs to gate this check
    /// on whether chain position is "known."
    last_timestamp: UnixTimeMillis,
    /// The in-progress historical sync attempt, if any (ADR-0044,
    /// "Decided: Replacing The Passive Jump For Real Catch-Up") —
    /// `Some` exactly while this node is fetching and verifying blocks
    /// it was not present for. While `Some`, `handle_peer_message`
    /// never feeds `ConsensusEngine` any live `Proposal`/`Vote`/
    /// `QuorumCertificateMessage` traffic at all (dropped, not
    /// queued) — this node has no business attempting a live round at
    /// a height it has not actually verified its way to yet.
    syncing: Option<SyncState>,
    /// `block_hash -> that proposal's own verified `BlockEnvelope``
    /// (ADR-0044, "Decided: Durable Finalized-Block Storage" —
    /// generalized from a header-only cache so the full envelope is
    /// available to persist as a [`FinalizedBlock`] the moment it
    /// finalizes), mirroring `ConsensusEngine`'s own internal
    /// `proposed_blocks` cache (the engine caches `transactions`; this
    /// node-layer cache exists because the engine has no field for
    /// `timestamp` at all, and `last_timestamp` needs the just-
    /// finalized block's own real value, not a recomputed one).
    /// Populated at the same two call sites as every
    /// `ConsensusEngine::handle_proposal` call (self-propose and
    /// verified-receive), and cleared the same way on a successful
    /// finalize — see [`act_on`]'s `Finalized` branch.
    pending_envelopes: HashMap<Digest, BlockEnvelope>,
    /// This node's own local transaction pool (ADR-0045, "Decided:
    /// Mempool Is Local Policy, Not Consensus") — never read by
    /// consensus logic itself yet (block building/validation are
    /// later passes); populated and drained only by the gossip
    /// handlers below.
    mempool: Mempool,
    /// This node's own last *finalized* height — distinct from
    /// `ctx.engine.state().height`, which is the height currently
    /// being contested (one past the last finalized one, once a round
    /// is under way) and, while [`Ctx::syncing`] is `Some`, frozen at
    /// whatever it was before the sync attempt started. `Mempool::
    /// admit`'s own validity-window/dry-run checks need "the height a
    /// transaction would actually be considered for next"
    /// (`last_finalized_height + 1`); the real historical sync
    /// machinery (ADR-0044) uses it as the one true measure of how far
    /// this node has actually, verifiably gotten, independent of
    /// whatever `ctx.engine` currently reflects.
    last_finalized_height: BlockHeight,
}

/// One in-progress historical sync attempt (ADR-0044, "Decided:
/// Replacing The Passive Jump For Real Catch-Up").
struct SyncState {
    /// The peer currently being asked for the next range — a
    /// `SyncBlockRangeResponse` from any other connection is
    /// unsolicited (this node did not ask it for anything) and
    /// ignored.
    peer_conn_id: u64,
    /// The highest height observed so far that started or extended
    /// this attempt. Keep requesting more ranges until this node's
    /// own `last_finalized_height` reaches it, or the peer currently
    /// being asked says it has nothing further to give — whichever
    /// comes first; a peer that is itself behind this value is not a
    /// failure, just a shorter attempt than originally hoped for.
    target_height: u64,
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
    let genesis_header = genesis_block_header(&manifest)?;
    let genesis_block_hash = genesis_header.block_hash()?;

    // ADR-0044, "Decided: Durable Finalized-Block Storage" -- a
    // restarting node's own data-dir may already hold real progress
    // from before it was killed. Resume from there, never
    // unconditionally from genesis: any real remaining gap to the
    // rest of the network (this node was down longer than its own
    // last real height) is what Syncing (`observe_height`) exists to
    // close, not something `run` should ever paper over by pretending
    // genesis is still the starting point.
    let (starting_height, last_block_hash, last_timestamp) = match store.latest_finalized_block()? {
        Some(block) => (
            block.envelope.header.height,
            block.envelope.block_hash()?,
            block.envelope.header.timestamp,
        ),
        None => (
            BlockHeight::GENESIS,
            genesis_block_hash,
            genesis_header.timestamp,
        ),
    };

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
        // One past whatever this node's own durable store says it
        // already really reached (genesis, height 0, if nothing --
        // ADR-0043), keeping `height = parent.height + 1` true across
        // both a genesis start and a real restart alike.
        engine: ConsensusEngine::new_height(BlockHeight::new(
            starting_height.get().saturating_add(1),
        )),
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
        last_block_hash,
        last_timestamp,
        syncing: None,
        pending_envelopes: HashMap::new(),
        mempool: Mempool::new(),
        // Whatever this node's own durable store says it already
        // really reached -- the last thing it has "finalized" before
        // any round begins this run; the first real transaction is
        // admitted against `last_finalized_height + 1`.
        last_finalized_height: starting_height,
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
                if !ctx.round_started
                    && ctx.syncing.is_none()
                    && ctx.established.len() >= required_peers
                {
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
                if ctx.syncing.is_none()
                    && is_current(ctx, height, round)
                    && ctx.engine.state().step == hn_consensus::ConsensusStep::Propose
                {
                    let action = ctx.engine.propose_timeout()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::PrevoteTimeout { height, round } => {
                if ctx.syncing.is_none()
                    && is_current(ctx, height, round)
                    && ctx.engine.state().step == hn_consensus::ConsensusStep::Prevote
                {
                    let action = ctx.engine.prevote_timeout()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::PrecommitTimeout { height, round } => {
                if ctx.syncing.is_none()
                    && is_current(ctx, height, round)
                    && ctx.engine.state().step == hn_consensus::ConsensusStep::Precommit
                {
                    let action = ctx.engine.precommit_timeout()?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::SyncRequestTimeout {
                peer_conn_id,
                from_height,
            } => {
                // Stale unless still syncing, still waiting on this
                // exact peer, and still at this exact height -- a
                // response that already arrived (or a peer switch
                // from some other cause) means this timer's own
                // request was already resolved, and firing it now
                // would otherwise wrongly abandon a perfectly good,
                // already-in-progress newer request.
                let is_current_wait = ctx
                    .syncing
                    .as_ref()
                    .is_some_and(|state| state.peer_conn_id == peer_conn_id)
                    && ctx.last_finalized_height.get() == from_height;
                if is_current_wait {
                    retry_sync_with_different_peer(ctx, peer_conn_id)?;
                }
            }
            NodeEvent::StartupGraceElapsed => {
                if !ctx.round_started && ctx.syncing.is_none() {
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
            observe_height(ctx, vote.payload.height.get(), conn_id)?;
            if ctx.syncing.is_some() {
                return Ok(());
            }
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
            observe_height(ctx, qc.height.get(), conn_id)?;
            if ctx.syncing.is_some() {
                return Ok(());
            }
            if expects_qc(ctx, &qc) {
                let action =
                    ctx.engine
                        .handle_quorum_certificate(qc, &ctx.store, &ctx.ordered_ids)?;
                act_on(ctx, action)?;
            }
        }
        (Channel::Consensus, MessageType::ConsensusProposal) => {
            let message = ConsensusProposalMessageV1::decode(&envelope.payload)?;
            let header = &message.envelope.header;
            observe_height(ctx, header.height.get(), conn_id)?;
            if ctx.syncing.is_some() {
                return Ok(());
            }
            // A proposal that does not target this engine's own exact
            // current (height, round, step) is either stale (a round
            // already left behind) or ahead of it -- either way it has
            // no defined transition from wherever the engine actually
            // is right now, so it is silently dropped rather than fed
            // to `ConsensusEngine::handle_proposal` (ADR-0034's own
            // closed, total `apply` would reject it as
            // `UnexpectedEvent`). `BlockHeader.height`/`.round`
            // (ADR-0039's own reason for self-describing height/round
            // survives the move from a bare top-level field into the
            // header itself, ADR-0042) make this an exact check, not
            // the weaker step-only guard this branch used before
            // ADR-0039.
            if header.height.get() == ctx.engine.state().height.get()
                && header.round.get() == ctx.engine.state().round.get()
                && ctx.engine.state().step == hn_consensus::ConsensusStep::Propose
            {
                // Receiver-side verification (ADR-0042, "Decided:
                // Receiver-Side Verification Rules") -- a failure here
                // is a dropped, logged message, never a fatal error:
                // this is externally-triggerable network input, and no
                // externally-triggerable input may crash this process.
                match verify_proposal(
                    &message,
                    ctx.chain_id,
                    ctx.network_id,
                    &ctx.ordered_ids,
                    &ctx.store,
                    &ctx.records,
                    (ctx.last_block_hash, ctx.last_timestamp),
                ) {
                    Ok(block_hash) => {
                        ctx.pending_envelopes
                            .insert(block_hash, message.envelope.clone());
                        let action = ctx.engine.handle_proposal(
                            block_hash,
                            message.envelope.body.transactions,
                            message.envelope.justification,
                        )?;
                        act_on(ctx, action)?;
                    }
                    Err(reason) => {
                        log_line(&format!(
                            "REJECTED proposal height={} round={} reason=\"{reason}\"",
                            header.height.get(),
                            header.round.get(),
                        ));
                    }
                }
            }
        }
        // ADR-0045, "Decided: Transaction Gossip" -- an announce/
        // request/response cycle over an opaque tx_id, mirroring
        // `Channel::Blocks`' own (dormant) `BlockAnnounce`/
        // `BlockRequest`/`BlockResponse` family exactly, since both are
        // the same "digest now, full content on request" shape. Every
        // step here is a dropped, logged message on failure, never a
        // fatal error -- this is externally-triggerable network input,
        // the same principle ADR-0042 already established for
        // proposals.
        (Channel::Transactions, MessageType::TransactionAnnounce) => {
            let announce = TransactionAnnounceV1::decode(&envelope.payload)?;
            if !ctx.mempool.contains(&announce.tx_id) {
                send_to(
                    ctx,
                    conn_id,
                    Channel::Transactions,
                    MessageType::TransactionRequest,
                    TransactionRequestV1 {
                        tx_id: announce.tx_id,
                    }
                    .encode(),
                )?;
            }
        }
        (Channel::Transactions, MessageType::TransactionRequest) => {
            let request = TransactionRequestV1::decode(&envelope.payload)?;
            if let Some(known) = ctx.mempool.get(&request.tx_id) {
                send_to(
                    ctx,
                    conn_id,
                    Channel::Transactions,
                    MessageType::TransactionResponse,
                    TransactionResponseV1 {
                        envelope: known.clone(),
                    }
                    .encode()?,
                )?;
            }
        }
        (Channel::Transactions, MessageType::TransactionResponse) => {
            let response = TransactionResponseV1::decode(&envelope.payload)?;
            let raw = response.envelope.encode()?;
            match ctx.mempool.admit(
                &raw,
                &ctx.store,
                ctx.last_finalized_height,
                ctx.chain_id,
                ctx.network_id,
                &ctx.records,
            ) {
                Ok(tx_id) => {
                    log_line(&format!("ADMITTED tx={}", hex(&tx_id)));
                    // Re-broadcast exactly once: every other node that
                    // already has this tx_id (including whichever peer
                    // sent it here) will see this announce, find it
                    // already admitted, and not re-request it --
                    // natural idempotency from `Mempool::contains`
                    // alone, not a separate "already forwarded" guard.
                    broadcast(
                        ctx,
                        Channel::Transactions,
                        MessageType::TransactionAnnounce,
                        TransactionAnnounceV1 { tx_id }.encode(),
                    )?;
                }
                Err(reason) => {
                    log_line(&format!("REJECTED transaction reason=\"{reason}\""));
                }
            }
        }
        // ADR-0044, "Decided: Wire Protocol" -- answers from this
        // node's own durable `FinalizedBlock` table directly, as
        // whatever contiguous prefix it actually has (never a gap or
        // reordering): `request.max_count` is clamped down to
        // `MAX_SYNC_BLOCKS_PER_RESPONSE` first so a requester asking
        // for more than one response may ever carry can never turn
        // into a response `SyncBlockRangeResponseV1::encode` itself
        // then has to reject.
        (Channel::Sync, MessageType::SyncBlockRangeRequest) => {
            let request = SyncBlockRangeRequestV1::decode(&envelope.payload)?;
            let max_count = request.max_count.min(MAX_SYNC_BLOCKS_PER_RESPONSE);
            let blocks = ctx
                .store
                .finalized_block_range(request.from_height, max_count)?;
            send_to(
                ctx,
                conn_id,
                Channel::Sync,
                MessageType::SyncBlockRangeResponse,
                SyncBlockRangeResponseV1 { blocks }.encode()?,
            )?;
        }
        (Channel::Sync, MessageType::SyncBlockRangeResponse) => {
            let Some(sync_state) = ctx.syncing.as_ref() else {
                // Not currently syncing at all -- unsolicited, ignore.
                return Ok(());
            };
            if sync_state.peer_conn_id != conn_id {
                // Not from the peer this node is currently waiting on
                // -- stale (already switched peers) or unsolicited,
                // either way ignore it rather than letting a second
                // peer's reply race the first.
                return Ok(());
            }
            let target_height = sync_state.target_height;
            let response = SyncBlockRangeResponseV1::decode(&envelope.payload)?;

            if response.blocks.is_empty() {
                // This peer has nothing further to give right now --
                // not a failure (it may simply be behind too), but
                // nothing was gained either; try a different peer in
                // case one exists with more.
                return retry_sync_with_different_peer(ctx, conn_id);
            }

            for block in &response.blocks {
                let last_accepted = (ctx.last_finalized_height, ctx.last_block_hash);
                match verify_and_commit_synced_block(
                    block,
                    ctx.chain_id,
                    ctx.network_id,
                    &ctx.ordered_ids,
                    last_accepted,
                    &mut ctx.store,
                    &ctx.records,
                ) {
                    Ok(block_hash) => {
                        ctx.last_finalized_height = block.envelope.header.height;
                        ctx.last_block_hash = block_hash;
                        ctx.last_timestamp = block.envelope.header.timestamp;
                        log_line(&format!(
                            "SYNCED height={} block={}",
                            ctx.last_finalized_height.get(),
                            hex(&block_hash),
                        ));
                    }
                    Err(reason) => {
                        log_line(&format!(
                            "REJECTED synced block height={} reason=\"{reason}\"",
                            block.envelope.header.height.get(),
                        ));
                        // This peer gave us something invalid -- never
                        // trust it again this attempt; try a different
                        // one starting from wherever this node
                        // genuinely got to.
                        return retry_sync_with_different_peer(ctx, conn_id);
                    }
                }
            }

            finish_or_continue_sync(ctx, conn_id, target_height)?;
        }
        _ => {}
    }
    Ok(())
}

/// If `observed_height` is strictly ahead of this engine's own
/// current height (the height it is actively trying to reach, one
/// past the last one it finalized, exactly `maybe_sync_forward`'s own
/// original trigger — same-height gossip about a round already in
/// progress is ordinary live consensus, not a gap), this node has
/// fallen behind the network's actual progress — most realistically
/// after restarting while its peers kept finalizing without it.
/// Unlike ADR-0039's own original "Decided: Passive Height-Observation
/// Catch-Up" (superseded, see ADR-0044's own "Decided: Replacing The
/// Passive Jump For Real Catch-Up"), this never jumps
/// `ConsensusEngine` straight to `observed_height` and never fakes a
/// chain position: it starts (or extends, if already in progress) a
/// real historical sync attempt, fetching and verifying every real
/// intervening block from `from_conn_id` — the peer whose own gossip
/// (`ConsensusVote`/`QuorumCertificate`/`ConsensusProposal` all
/// self-report `height`) revealed the gap in the first place.
/// `ctx.engine` itself is never touched while a sync attempt is in
/// progress (frozen at whatever height it was already at), so this
/// same comparison keeps working for a second, higher observation
/// that arrives mid-attempt.
///
/// Never triggered by an already-caught-up or genuinely stale (at or
/// behind this engine's own current height) observation —
/// `observed_height` must be strictly greater.
fn observe_height(ctx: &mut Ctx, observed_height: u64, from_conn_id: u64) -> NodeResult<()> {
    if observed_height <= ctx.engine.state().height.get() {
        return Ok(());
    }
    match &mut ctx.syncing {
        Some(state) => {
            // Already fetching -- just remember the higher target, if
            // any; the in-flight request/response cycle (or its own
            // timeout) is what actually drives this forward, not a
            // second, redundant request fired on every single gossip
            // message that happens to arrive while busy.
            state.target_height = state.target_height.max(observed_height);
            Ok(())
        }
        None => start_sync(ctx, from_conn_id, observed_height),
    }
}

/// Starts a brand-new historical sync attempt (ADR-0044, "Decided:
/// Replacing The Passive Jump For Real Catch-Up") against
/// `peer_conn_id`, from this node's own current `last_finalized_height`
/// toward `target_height`.
fn start_sync(ctx: &mut Ctx, peer_conn_id: u64, target_height: u64) -> NodeResult<()> {
    log_line(&format!(
        "SYNCING from height={} toward observed_height={target_height} (peer_conn_id={peer_conn_id})",
        ctx.last_finalized_height.get(),
    ));
    ctx.syncing = Some(SyncState {
        peer_conn_id,
        target_height,
    });
    request_next_sync_range(ctx, peer_conn_id)
}

/// A per-peer request timeout, and how many times to keep retrying
/// against whichever peer is currently being asked, before giving up
/// on *that* peer and switching to another one (ADR-0044's own
/// addendum, "Плюс тайм-ауты запросов и смена пира при ошибке" —
/// named here as the real implementation constants that decision
/// calls for, not a value the ADR itself fixes). A generous multiple
/// of `base_timeout_ms`: fetching and verifying a real batch of
/// blocks genuinely takes longer than one consensus round's own
/// propose/prevote/precommit timeout.
const SYNC_REQUEST_TIMEOUT_MULTIPLIER: u64 = 20;

/// Sends one `SyncBlockRangeRequestV1` to `peer_conn_id`, starting
/// immediately after this node's own current `last_finalized_height`,
/// and schedules the timeout that fires if no matching
/// `SyncBlockRangeResponse` ever arrives.
fn request_next_sync_range(ctx: &mut Ctx, peer_conn_id: u64) -> NodeResult<()> {
    let from_height = ctx.last_finalized_height;
    send_to(
        ctx,
        peer_conn_id,
        Channel::Sync,
        MessageType::SyncBlockRangeRequest,
        SyncBlockRangeRequestV1 {
            from_height,
            max_count: MAX_SYNC_BLOCKS_PER_RESPONSE,
        }
        .encode(),
    )?;
    spawn_after(
        ctx.event_tx.clone(),
        Duration::from_millis(
            ctx.base_timeout_ms
                .saturating_mul(SYNC_REQUEST_TIMEOUT_MULTIPLIER),
        ),
        NodeEvent::SyncRequestTimeout {
            peer_conn_id,
            from_height: from_height.get(),
        },
    );
    Ok(())
}

/// Picks any currently-established peer other than `exclude` to try
/// next — falls back to retrying `exclude` itself if it is genuinely
/// the only peer currently established (a small devnet has no one
/// else to ask), rather than stalling forever waiting for a second
/// peer that may not exist.
fn pick_sync_peer(ctx: &Ctx, exclude: u64) -> Option<u64> {
    ctx.established
        .iter()
        .copied()
        .find(|&conn_id| conn_id != exclude)
        .or_else(|| ctx.established.contains(&exclude).then_some(exclude))
}

/// The peer currently being asked failed this attempt (an invalid
/// block, a decode error, or its own request timing out) — per
/// ADR-0044's own "Decided: Verification Procedure," per-peer failure
/// is never fatal to the sync attempt as a whole: try a different
/// established peer, continuing from wherever this node's own
/// `last_finalized_height` genuinely got to. If no other peer is even
/// established right now, the attempt simply pauses (`ctx.syncing =
/// None`) rather than retrying nothing forever; the next real gossip
/// message from any peer (once one exists) re-triggers
/// [`observe_height`] and starts over.
fn retry_sync_with_different_peer(ctx: &mut Ctx, failed_conn_id: u64) -> NodeResult<()> {
    let Some(next_peer) = pick_sync_peer(ctx, failed_conn_id) else {
        ctx.syncing = None;
        return Ok(());
    };
    if let Some(state) = &mut ctx.syncing {
        state.peer_conn_id = next_peer;
    }
    request_next_sync_range(ctx, next_peer)
}

/// After a successful `SyncBlockRangeResponse` batch: if this node's
/// own `last_finalized_height` has reached `target_height`, the
/// attempt is complete — leave `Syncing` and begin a real round at the
/// next height, the exact same way `run()` itself starts one at
/// startup. Otherwise, there is more to fetch and `peer_conn_id` is
/// responding correctly, so simply ask it for the next range.
fn finish_or_continue_sync(ctx: &mut Ctx, peer_conn_id: u64, target_height: u64) -> NodeResult<()> {
    if ctx.last_finalized_height.get() >= target_height {
        ctx.syncing = None;
        log_line(&format!(
            "SYNC COMPLETE height={}",
            ctx.last_finalized_height.get()
        ));
        ctx.engine = ConsensusEngine::new_height(BlockHeight::new(
            ctx.last_finalized_height.get().saturating_add(1),
        ));
        ctx.round_started = true;
        let action = ctx.engine.begin_round()?;
        act_on(ctx, action)
    } else {
        request_next_sync_range(ctx, peer_conn_id)
    }
}

/// Verifies an incoming [`ConsensusProposalMessageV1`] before it is
/// ever handed to [`ConsensusEngine::handle_proposal`] (ADR-0042,
/// "Decided: Receiver-Side Verification Rules") — returns the
/// envelope's own, independently recomputed `block_hash` on success.
///
/// A failure here is a dropped message, not a fatal error: this is
/// externally-triggerable network input — no externally-triggerable
/// input may ever crash this process, the same principle ADR-0038's
/// own "no-panic discipline" already applies at the process boundary,
/// now applied one layer in, at the message-verification boundary.
/// (Decoding `envelope.payload` itself can still propagate a fatal
/// error on malformed bytes, same as every other message type already
/// does in `handle_peer_message` — a real, separate, broader decode-
/// resilience gap this ADR does not attempt to close.)
///
/// Checks, in order: header hash recomputation (implicit —
/// `block_hash` is derived, never trusted as a separately-carried
/// claim), chain/network binding, proposer eligibility
/// (`round_proposer`), proposer signature (against that proposer's own
/// registered consensus key), header-vs-body consistency
/// (`BlockEnvelope::verify_body_matches_header`), a *real* re-execution
/// of `body.transactions` against `store` (ADR-0045, "Decided:
/// Validator Execution") — the body's own claimed `receipts` must
/// match that execution's real result exactly, and `header.state_root`
/// must match the real post-block root it produces — this devnet's own
/// fixed `consensus_root`/`protocol_parameters_hash` values,
/// `parent_block_hash`/timestamp monotonicity against
/// `last_block_hash`/`last_timestamp` (unconditional now, ADR-0044:
/// there is no longer a passive jump that could leave those stale —
/// every height this node reaches is either locally finalized or
/// reached via full verified replay), and finally the embedded
/// `justification` quorum certificate's own signatures, if present.
///
/// Takes only the specific pieces of [`Ctx`] this check actually
/// needs, rather than the whole context, so it can be exercised
/// directly in a unit test without constructing a full `Ctx`'s own
/// sockets/channels/handshake state — none of which this function
/// touches.
fn verify_proposal(
    message: &ConsensusProposalMessageV1,
    chain_id: u8,
    network_id: u16,
    ordered_ids: &[Digest],
    store: &(impl StateReader + StateEnumerator),
    validator_candidates: &[ValidatorRecordV1],
    last_accepted: (Digest, UnixTimeMillis),
) -> Result<Digest, String> {
    let (last_block_hash, last_timestamp) = last_accepted;
    let header = &message.envelope.header;
    let block_hash = message
        .envelope
        .block_hash()
        .map_err(|error| format!("header hash computation failed: {error}"))?;

    if header.chain_id != chain_id || header.network_id != network_id {
        return Err("header chain_id/network_id does not match this node's own".to_string());
    }

    if round_proposer(ordered_ids, header.height, header.round) != Some(header.proposer) {
        return Err(format!(
            "{} is not this round's legitimate proposer",
            hex(&header.proposer)
        ));
    }

    let key = active_key(store, &header.proposer)
        .map_err(|error| format!("could not resolve the proposer's registered key: {error}"))?
        .ok_or_else(|| "the proposer has no registered consensus key".to_string())?;
    message
        .proposer_signature
        .verify(&key, &block_hash)
        .map_err(|error| format!("proposer signature invalid: {error}"))?;

    message
        .envelope
        .verify_body_matches_header()
        .map_err(|error| format!("body does not match header: {error}"))?;

    // ADR-0045, "Decided: Validator Execution" -- actually run the
    // proposed transactions, rather than trusting the body's own
    // claimed receipts/state_root: `verify_body_matches_header` above
    // only checked that the body is internally self-consistent with
    // the header, not that it reflects what really applying it
    // produces.
    let application = apply_block(
        &message.envelope.body.transactions,
        store,
        header.height,
        validator_candidates,
    )
    .map_err(|error| format!("block application failed: {error}"))?;
    let actual_receipts: Vec<ReceiptV1> = application
        .applied
        .iter()
        .map(|applied| applied.receipt)
        .collect();
    if actual_receipts != message.envelope.body.receipts {
        return Err("receipts do not match the real execution result".to_string());
    }
    let write_set: Vec<Write> = application
        .applied
        .iter()
        .flat_map(|applied| applied.write_set.clone())
        .collect();
    let empty_table = EmptyHashTable::build()
        .map_err(|error| format!("could not build the empty-subtree hash table: {error}"))?;
    let state_enumerator = ExcludingGenesisMarker {
        inner: store,
        excluded: genesis_marker_key()
            .map_err(|error| format!("could not compute the genesis marker key: {error}"))?,
    };
    let actual_state_root = compute_post_state_root(&state_enumerator, &write_set, &empty_table)
        .map_err(|error| format!("could not compute the real post-block state_root: {error}"))?;
    if header.state_root != actual_state_root {
        return Err("state_root does not match the real post-block state".to_string());
    }
    if header.consensus_root != DEVNET_VSC {
        return Err("consensus_root does not match this devnet's own fixed value".to_string());
    }
    let expected_protocol_parameters_hash = protocol_parameters_placeholder_hash()
        .map_err(|error| format!("could not compute the expected placeholder: {error}"))?;
    if header.protocol_parameters_hash != expected_protocol_parameters_hash {
        return Err("protocol_parameters_hash does not match the expected placeholder".to_string());
    }

    if header.parent_block_hash != last_block_hash {
        return Err(
            "parent_block_hash does not match this node's own last finalized block".to_string(),
        );
    }
    if header.timestamp.as_millis() <= last_timestamp.as_millis() {
        return Err("timestamp does not strictly advance past the parent's own".to_string());
    }

    if let Some(qc) = &message.envelope.justification {
        qc.verify_signatures(store, ordered_ids)
            .map_err(|error| format!("justification quorum certificate invalid: {error}"))?;
    }

    Ok(block_hash)
}

/// Verifies a [`FinalizedBlock`] fetched during historical sync
/// (ADR-0044, "Decided: Verification Procedure") against this node's
/// own last accepted `(height, block_hash)` and the active set at that
/// height, replays it for real, and — only on full success — commits
/// it atomically together with its own record via
/// [`BlockStore::commit_block`], the exact same one-transaction
/// guarantee a live-consensus-produced block already gets
/// ([`hn_consensus::ConsensusEngine::commit_finalized_block`],
/// ADR-0044 step 3). Returns the block's own `block_hash` on success.
///
/// Checks, in order: header hash recomputation (implicit, same
/// principle as [`verify_proposal`]), chain/network binding, sequential
/// height with a matching `parent_block_hash` (checked unconditionally
/// — full replay from genesis means this chain of equalities is never
/// skipped), header-vs-body consistency
/// (`BlockEnvelope::verify_body_matches_header`), `commit_qc` itself
/// (a real `precommit` targeting exactly this block/height, verified
/// via `QuorumCertificate::verify_signatures` against the active set —
/// the one requirement live proposal verification does not have: a
/// node fetching history was never there to watch real-time votes
/// accumulate into a quorum, so the certificate itself is the only
/// proof available), and finally a real re-execution of
/// `envelope.body.transactions` whose `receipts`/`state_root` must
/// match the header's own claimed values exactly (the same
/// "recompute, don't trust" check [`verify_proposal`] already applies
/// to a live proposal).
///
/// A failure here is a dropped, logged sync attempt against that
/// specific peer, never a fatal error — the same externally-
/// triggerable-input principle ADR-0042 already established, now
/// applied to historical rather than live data; the caller tries a
/// different peer for the remaining range (ADR-0044, "Decided:
/// Verification Procedure").
fn verify_and_commit_synced_block<S: StateReader + StateEnumerator + BlockStore>(
    block: &FinalizedBlock,
    chain_id: u8,
    network_id: u16,
    ordered_ids: &[Digest],
    last_accepted: (BlockHeight, Digest),
    store: &mut S,
    validator_candidates: &[ValidatorRecordV1],
) -> Result<Digest, String> {
    let header = &block.envelope.header;
    let block_hash = block
        .envelope
        .block_hash()
        .map_err(|error| format!("header hash computation failed: {error}"))?;

    if header.chain_id != chain_id || header.network_id != network_id {
        return Err("header chain_id/network_id does not match this node's own".to_string());
    }

    let (last_height, last_block_hash) = last_accepted;
    let expected_height = BlockHeight::new(last_height.get().saturating_add(1));
    if header.height != expected_height {
        return Err(format!(
            "expected the next sequential height {}, found {}",
            expected_height.get(),
            header.height.get()
        ));
    }
    if header.parent_block_hash != last_block_hash {
        return Err(
            "parent_block_hash does not match this node's own last accepted block (foreign chain)"
                .to_string(),
        );
    }

    block
        .envelope
        .verify_body_matches_header()
        .map_err(|error| format!("body does not match header: {error}"))?;

    let commit_qc = &block.commit_qc;
    if commit_qc.certificate_type != VoteType::Precommit {
        return Err("commit_qc is not a precommit certificate".to_string());
    }
    if commit_qc.target_type != VoteTargetType::Block || commit_qc.target_hash != block_hash {
        return Err("commit_qc does not target this exact block".to_string());
    }
    if commit_qc.height != header.height {
        return Err("commit_qc does not target this exact height".to_string());
    }
    commit_qc
        .verify_signatures(store, ordered_ids)
        .map_err(|error| format!("commit_qc invalid: {error}"))?;

    // ADR-0044, "Decided: Verification Procedure", step 5 -- a real
    // re-execution, not merely a re-check of internal consistency
    // (the `verify_body_matches_header` call above already did that
    // against the body's own *claimed* receipts/roots).
    let application = apply_block(
        &block.envelope.body.transactions,
        store,
        header.height,
        validator_candidates,
    )
    .map_err(|error| format!("block application failed: {error}"))?;
    let actual_receipts: Vec<ReceiptV1> = application
        .applied
        .iter()
        .map(|applied| applied.receipt)
        .collect();
    if actual_receipts != block.envelope.body.receipts {
        return Err("receipts do not match the real execution result".to_string());
    }
    let writes: Vec<Write> = application
        .applied
        .iter()
        .flat_map(|applied| applied.write_set.clone())
        .collect();
    let empty_table = EmptyHashTable::build()
        .map_err(|error| format!("could not build the empty-subtree hash table: {error}"))?;
    let actual_state_root = {
        let state_enumerator = ExcludingGenesisMarker {
            inner: &*store,
            excluded: genesis_marker_key()
                .map_err(|error| format!("could not compute the genesis marker key: {error}"))?,
        };
        compute_post_state_root(&state_enumerator, &writes, &empty_table)
            .map_err(|error| format!("could not compute the real post-block state_root: {error}"))?
    };
    if header.state_root != actual_state_root {
        return Err("state_root does not match the real post-block state".to_string());
    }

    store
        .commit_block(&writes, block)
        .map_err(|error| format!("could not durably commit the synced block: {error}"))?;

    Ok(block_hash)
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

/// Sends one message to exactly one established peer — [`broadcast`]'s
/// directed counterpart, needed for `TransactionRequest`/
/// `TransactionResponse` (ADR-0045, "Decided: Transaction Gossip"):
/// those answer the specific peer that asked, not every peer.
fn send_to(
    ctx: &Ctx,
    conn_id: u64,
    channel: Channel,
    message_type: MessageType,
    payload: Vec<u8>,
) -> NodeResult<()> {
    let Some(link) = ctx.links.get(&conn_id) else {
        return Ok(());
    };
    let envelope = P2PMessageEnvelopeV1::new(
        PROTOCOL_VERSION,
        ctx.chain_id,
        ctx.network_id,
        channel,
        message_type,
        payload,
    )?;
    let _ = link.send(envelope);
    Ok(())
}

fn act_on(ctx: &mut Ctx, action: ConsensusAction) -> NodeResult<()> {
    match action {
        ConsensusAction::None => maybe_start_propose_stage(ctx)?,
        ConsensusAction::Prevote(target) => cast_vote(ctx, VoteType::Prevote, target)?,
        ConsensusAction::Precommit(target) => cast_vote(ctx, VoteType::Precommit, target)?,
        ConsensusAction::Finalized {
            block_hash,
            commit_qc,
        } => {
            let height = ctx.engine.state().height;
            let round = ctx.engine.state().round;
            // `pending_envelopes` and the engine's own `proposed_blocks`
            // cache are populated together at the same two call sites
            // (the proposer branch and the receiver branch's verified-
            // proposal path), so they agree for as long as one
            // `ConsensusEngine` lives through an uninterrupted
            // sequence of rounds. But a node that just finished a real
            // sync attempt can still miss the very first live proposal
            // at its own resumption height: that proposal may have
            // been broadcast *while* this node was still busy fetching
            // and replaying its own sync batch, not listening for
            // ordinary consensus gossip at all -- yet it can perfectly
            // well catch that same round's own `precommit` quorum
            // certificate afterward, which carries no proposal content
            // of its own. This is the exact race ADR-0039's own passive
            // catch-up used to name; real sync narrows the window a lot
            // (most of this node's own history is now genuinely
            // verified, not merely assumed) but does not close this
            // one specific edge entirely, so it is still a dropped,
            // logged event here, never a fatal error.
            let Some(envelope) = ctx.pending_envelopes.get(&block_hash).cloned() else {
                log_line(&format!(
                    "SKIPPED finalizing height={} round={} block={} (no cached proposal)",
                    height.get(),
                    round.get(),
                    hex(&block_hash),
                ));
                return Ok(());
            };
            match ctx.engine.commit_finalized_block(
                block_hash,
                envelope,
                commit_qc,
                &mut ctx.store,
                &ctx.records,
            ) {
                Ok(result) => {
                    // The real post-commit state_root -- `ctx.store`
                    // already holds this block's own committed writes
                    // by this point, so re-enumerating it now (rather
                    // than threading the proposer's own already-
                    // computed value through) proves this
                    // independently, the same "recompute, don't trust"
                    // principle `verify_proposal` itself already
                    // applies.
                    let empty_table = EmptyHashTable::build()?;
                    let state_enumerator = ExcludingGenesisMarker {
                        inner: &ctx.store,
                        excluded: genesis_marker_key()?,
                    };
                    let state_root = compute_post_state_root(&state_enumerator, &[], &empty_table)?;
                    log_line(&format!(
                        "FINALIZED height={} round={} block={} applied={} state_root={}",
                        height.get(),
                        round.get(),
                        hex(&block_hash),
                        result.applied.len(),
                        hex(&state_root)
                    ));
                    ctx.last_block_hash = block_hash;
                    // `pending_envelopes` mirrors the engine's own
                    // `proposed_blocks` cache (ADR-0042) -- the just-
                    // finalized block's own real `timestamp` becomes
                    // the new monotonicity floor, and every other
                    // cached envelope was for a competing, now-
                    // irrelevant round at this same height, exactly
                    // like `commit_finalized_block`'s own
                    // `proposed_blocks.clear()`.
                    if let Some(envelope) = ctx.pending_envelopes.remove(&block_hash) {
                        ctx.last_timestamp = envelope.header.timestamp;
                    }
                    ctx.pending_envelopes.clear();
                    ctx.last_finalized_height = height;
                    // ADR-0045, "Decided: Commit" -- once a block is
                    // actually finalized, every transaction it really
                    // included is done, successfully or not (a
                    // `Failed` receipt still consumed its own nonce);
                    // either way it must never be selected into a
                    // future block again.
                    ctx.mempool
                        .remove_included(result.applied.iter().map(|applied| applied.tx_id));
                    let action = ctx.engine.begin_new_height()?;
                    act_on(ctx, action)?;
                }
                // The engine's own `proposed_blocks` cache can miss
                // this exact same way, for the exact same reason, even
                // when `pending_envelopes` itself did not -- a node's
                // own `handle_proposal` call is what populates both
                // together, so if that call itself never happened for
                // this block, neither cache has it.
                Err(ConsensusError::UnknownProposedBlock { block_hash }) => {
                    log_line(&format!(
                        "SKIPPED finalizing height={} round={} block={} (no cached proposal)",
                        height.get(),
                        round.get(),
                        hex(&block_hash),
                    ));
                }
                Err(error) => return Err(error.into()),
            }
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
        // ADR-0045, "Decided: Block Building (Proposer)" -- select
        // from this node's own mempool, then actually run the result
        // through `apply_block` for real (not a second, separate
        // simulation): `select_for_block` already ran this exact check
        // per candidate, but re-deriving `result` from the final
        // selection here, rather than threading it out of
        // `select_for_block` itself, keeps "what got selected" and
        // "what the real receipts/roots are" from two different
        // internal code paths that could silently drift apart.
        let selected = ctx
            .mempool
            .select_for_block(&ctx.store, height, &ctx.records);
        let application = apply_block(&selected, &ctx.store, height, &ctx.records)?;
        let empty_table = EmptyHashTable::build()?;
        let write_set: Vec<Write> = application
            .applied
            .iter()
            .flat_map(|applied| applied.write_set.clone())
            .collect();
        let state_enumerator = ExcludingGenesisMarker {
            inner: &ctx.store,
            excluded: genesis_marker_key()?,
        };
        let state_root = compute_post_state_root(&state_enumerator, &write_set, &empty_table)?;

        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions: selected,
            receipts: application
                .applied
                .iter()
                .map(|applied| applied.receipt)
                .collect(),
            evidence: Vec::new(),
            extra_data: Vec::new(),
        };
        let header = build_block_header(ctx, height, round, &body, state_root)?;
        let block_hash = header.block_hash()?;
        // `body` moves into `envelope` just below; the engine's own
        // `proposed_blocks` cache (consulted later by
        // `commit_finalized_block`, once this block actually
        // finalizes) needs its own, separate copy of exactly the same
        // transactions.
        let transactions_for_engine = body.transactions.clone();
        let envelope = BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            // Re-proposing a value this node is itself locked on under
            // a real `prevote` quorum certificate (`hn_consensus::
            // ConsensusState`'s own "Locking rule") is not wired here
            // -- unchanged from before this pass, a distinct, separate
            // gap this ADR does not attempt to close.
            justification: None,
        };
        ctx.pending_envelopes.insert(block_hash, envelope.clone());
        let proposer_signature = SignatureEnvelope {
            algorithm_id: ctx.consensus_keypair.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: ctx.consensus_keypair.sign(&block_hash).to_vec(),
        };
        broadcast(
            ctx,
            Channel::Consensus,
            MessageType::ConsensusProposal,
            ConsensusProposalMessageV1 {
                envelope,
                proposer_signature,
            }
            .encode()?,
        )?;
        let action = ctx
            .engine
            .handle_proposal(block_hash, transactions_for_engine, None)?;
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

/// Builds this round's real `hn_state::BlockHeader` (ADR-0040, "Decided:
/// Real `block_hash` In `hn-node`") — replaces the earlier synthetic,
/// non-cryptographic per-round identifier ADR-0037's own devnet
/// scaffolding used. Every field is either real local state, an
/// already-decided placeholder this codebase already established
/// elsewhere, or a value that is genuinely, currently correct for this
/// devnet's own always-empty blocks — see each field's own comment
/// below for which.
/// Builds this round's real `hn_state::BlockHeader` from `body` and its
/// own already-computed `state_root` (ADR-0045, "Decided: Block
/// Building (Proposer)") — `transactions_root`/`receipts_root`/
/// `evidence_root`/`extra_data_hash` are derived from `body` itself
/// (via `BlockBody`'s own helper methods, the same ones `verify_proposal`
/// independently recomputes on receipt), not assumed empty: this
/// devnet's blocks are no longer always empty now that real
/// transactions can flow through the mempool (ADR-0045), even though
/// an empty selection remains a genuinely valid, common case.
fn build_block_header(
    ctx: &Ctx,
    height: BlockHeight,
    round: Round,
    body: &BlockBody,
    state_root: Digest,
) -> NodeResult<BlockHeader> {
    let now_millis = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX);

    Ok(BlockHeader {
        header_version: HEADER_VERSION_1,
        chain_id: ctx.chain_id,
        network_id: ctx.network_id,
        height,
        round,
        // No real consensus-protocol epoch rotation exists in this
        // devnet yet -- matches the fixed `Epoch::new(0)` already used
        // for every vote this node casts (`cast_vote`, below).
        epoch: Epoch::new(0),
        // No hard-fork/activation signaling exists in this devnet yet.
        protocol_epoch: ProtocolEpoch::GENESIS,
        parent_block_hash: ctx.last_block_hash,
        // No `validator_address_body` derivation is established for
        // `validator_id` anywhere in this project (ADR-0010 never
        // defines one, and this devnet's own `validator_id` is itself
        // "taken as given," ADR-0038) -- this validator's own already-
        // established identity bytes are the most correct value
        // available, not a placeholder standing in for a real
        // derivation this codebase doesn't have.
        proposer: ctx.own_validator_id,
        timestamp: UnixTimeMillis::from_millis(now_millis),
        transactions_root: body.transactions_root()?,
        // Real, independently-recomputable now (ADR-0045) --
        // `compute_post_state_root` over this node's own committed
        // state, overlaid with this exact block's own write set, not
        // assumed unchanging from genesis anymore.
        state_root,
        receipts_root: body.receipts_root()?,
        // No `events` concept exists in `BlockBody` at all yet
        // (HNVM-gated, ADR-0008) -- `list_empty_root()` remains the
        // one correct value for as long as that stays true, regardless
        // of whether `transactions` is empty.
        events_root: list_empty_root()?,
        // The same fixed `DEVNET_VSC` this node's own votes/QCs already
        // use as `validator_set_commitment` -- `consensus_root` and
        // `validator_set_commitment` are decided to be the exact same
        // value (ADR-0010), so reusing this constant here keeps that
        // equality genuinely true, not coincidentally matching.
        consensus_root: DEVNET_VSC,
        evidence_root: body.evidence_root()?,
        protocol_parameters_hash: protocol_parameters_placeholder_hash()?,
        extra_data_hash: body.extra_data_hash()?,
    })
}

/// Genesis's own real `BlockHeader` (ADR-0043, "Genesis As A Real
/// Block 0") — every node constructs this identically from the exact
/// same, already `genesis_hash`-verified `GenesisManifest` (ADR-0038),
/// not a consensus-voted, gossiped, or sentinel value. `BlockHeight::
/// GENESIS` (0) is genesis's own, sole height — the first real
/// consensus-produced block now starts at height 1 (`run`'s own
/// `ConsensusEngine::new_height` call), so `height = parent.height + 1`
/// holds across this boundary too, not just between real blocks.
///
/// This header's own `block_hash` becomes `Ctx.last_block_hash`'s real
/// starting value (`run`, below), replacing the previous `[0; 32]`
/// placeholder — `verify_proposal`'s own `parent_block_hash` check
/// (ADR-0042) is now meaningful from the very first real block onward:
/// every node independently computes the exact same hash from the
/// exact same genesis content, so a wrong claimed parent is now
/// actually detectable, not merely checked against an arbitrary shared
/// convention nothing could ever really get wrong.
fn genesis_block_header(manifest: &GenesisManifest) -> NodeResult<BlockHeader> {
    let body = BlockBody {
        body_version: BODY_VERSION_1,
        transactions: Vec::new(),
        receipts: Vec::new(),
        evidence: Vec::new(),
        extra_data: manifest.extra_data.clone(),
    };
    Ok(BlockHeader {
        header_version: HEADER_VERSION_1,
        chain_id: manifest.chain_id,
        network_id: manifest.network_id,
        height: BlockHeight::GENESIS,
        // Genesis is not reached through any real round -- `0` names
        // "no round," not a claim about which round produced it.
        round: Round::new(0),
        epoch: Epoch::new(0),
        protocol_epoch: ProtocolEpoch::GENESIS,
        // Genesis has no predecessor at all -- this is the chain's own
        // actual first content, not a block with something before it.
        parent_block_hash: [0_u8; 32],
        // No validator proposes genesis -- it is the protocol's own
        // fixed, published data (`BlockHeader.proposer`'s own doc
        // comment already allows "the proposing validator OR SYSTEM
        // AUTHORITY's own address_body"); `[0; 32]` names "no
        // validator," not a placeholder standing in for one.
        proposer: [0_u8; 32],
        // `genesis_time` is Unix seconds (ADR-0038); `BlockHeader.
        // timestamp` is `UnixTimeMillis` (ADR-0008) -- converted once,
        // here, not left as a silent unit mismatch.
        timestamp: UnixTimeMillis::from_millis(manifest.genesis_time.saturating_mul(1000)),
        transactions_root: body.transactions_root()?,
        state_root: manifest.initial_state_root()?,
        receipts_root: body.receipts_root()?,
        events_root: list_empty_root()?,
        // The same fixed `DEVNET_VSC` every other block in this devnet
        // already uses (ADR-0010's own `consensus_root ==
        // validator_set_commitment` equality, kept genuinely true) --
        // not a separately-computed "real" value just for genesis,
        // which would make genesis's own `consensus_root` inconsistent
        // with every block that follows it.
        consensus_root: DEVNET_VSC,
        evidence_root: body.evidence_root()?,
        protocol_parameters_hash: protocol_parameters_placeholder_hash()?,
        extra_data_hash: body.extra_data_hash()?,
    })
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

#[cfg(test)]
mod tests {
    use hn_core::{AccountNonce, BlockHeight, Round};
    use hn_crypto::account_address_body;
    use hn_state::{
        AccessListV1, AccountSection, BalanceValueV1, FINALIZED_BLOCK_VERSION_1, ReceiptStatus,
        StateWriter, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType,
        ValidatorSection, ValidityWindowV1, account_section_state_key, extra_data_hash,
        validator_section_state_key,
    };
    use hn_storage::InMemoryStateStore;

    use super::*;
    use crate::genesis::{GenesisAccount, GenesisAllocations, GenesisValidator};

    const CHAIN_ID: u8 = 1;
    const NETWORK_ID: u16 = 1;
    const HEIGHT: u64 = 5;
    const ROUND: u64 = 0;

    /// `round_proposer([id_a, id_b], height=5, round=0)` selects index
    /// `(5 + 0) % 2 == 1` — validator B, by construction, so these
    /// fixtures exercise the real eligibility formula rather than
    /// assuming a result.
    struct Fixture {
        store: InMemoryStateStore,
        ordered_ids: Vec<Digest>,
        legitimate_proposer: Ed25519KeyPair,
        legitimate_proposer_id: Digest,
    }

    fn fixture() -> NodeResult<Fixture> {
        let mut store = InMemoryStateStore::new();
        let id_a = [0x10_u8; 32];
        let id_b = [0x20_u8; 32];
        let keypair_a = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, id_a);
        let keypair_b = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, id_b);
        for (validator_id, keypair) in [(id_a, &keypair_a), (id_b, &keypair_b)] {
            let record = ValidatorRecordV1 {
                validator_id,
                consensus_key: keypair.key_descriptor(),
                bonded_stake: 100,
                voting_power: 100,
                status: ValidatorStatus::Active,
                pending_unbonding: None,
            };
            let key = validator_section_state_key(&validator_id, ValidatorSection::Record)?;
            store.set(key, record.encode()?)?;
        }
        Ok(Fixture {
            store,
            ordered_ids: vec![id_a, id_b],
            legitimate_proposer: keypair_b,
            legitimate_proposer_id: id_b,
        })
    }

    /// A self-consistent, correctly-signed envelope for `proposer_id`/
    /// `keypair`, actually executing `transactions` against `store` to
    /// derive real `receipts`/`state_root` — every check
    /// `verify_proposal` can satisfy without external context already
    /// holds; callers tamper one field at a time (or pass a
    /// `transactions` list that cannot really apply) to exercise one
    /// specific rejection.
    fn signed_envelope(
        store: &InMemoryStateStore,
        proposer_id: Digest,
        keypair: &Ed25519KeyPair,
        parent_block_hash: Digest,
        timestamp_millis: u64,
        transactions: Vec<TransactionEnvelope>,
    ) -> NodeResult<(BlockEnvelope, SignatureEnvelope)> {
        let application = apply_block(&transactions, store, BlockHeight::new(HEIGHT), &[])?;
        let receipts: Vec<ReceiptV1> = application
            .applied
            .iter()
            .map(|applied| applied.receipt)
            .collect();
        let write_set: Vec<Write> = application
            .applied
            .iter()
            .flat_map(|applied| applied.write_set.clone())
            .collect();
        let empty_table = EmptyHashTable::build()?;
        let state_root = compute_post_state_root(store, &write_set, &empty_table)?;

        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions,
            receipts,
            evidence: Vec::new(),
            extra_data: Vec::new(),
        };
        let header = BlockHeader {
            header_version: HEADER_VERSION_1,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            height: BlockHeight::new(HEIGHT),
            round: Round::new(ROUND),
            epoch: Epoch::new(0),
            protocol_epoch: ProtocolEpoch::GENESIS,
            parent_block_hash,
            proposer: proposer_id,
            timestamp: UnixTimeMillis::from_millis(timestamp_millis),
            transactions_root: body.transactions_root()?,
            state_root,
            receipts_root: body.receipts_root()?,
            events_root: list_empty_root()?,
            consensus_root: DEVNET_VSC,
            evidence_root: body.evidence_root()?,
            protocol_parameters_hash: protocol_parameters_placeholder_hash()?,
            extra_data_hash: extra_data_hash(&[])?,
        };
        let envelope = BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification: None,
        };
        let block_hash = envelope.block_hash()?;
        let signature = SignatureEnvelope {
            algorithm_id: keypair.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: keypair.sign(&block_hash).to_vec(),
        };
        Ok((envelope, signature))
    }

    const TRANSFER_RECIPIENT: Digest = [0x50; 32];

    /// A real, correctly-signed native transfer from `sender`/
    /// `keypair` at `nonce` — bootstraps `sender`'s own identity (no
    /// test fixture ever pre-populates one), so only valid as the
    /// *first* transaction a given sender's own tests apply.
    fn signed_transaction(
        keypair: &Ed25519KeyPair,
        sender: Digest,
        nonce: u64,
    ) -> NodeResult<TransactionEnvelope> {
        let payload = TransferPayloadV1 {
            recipient: TRANSFER_RECIPIENT,
            asset_id: None,
            amount: 10,
        }
        .encode()?;
        let mut envelope = TransactionEnvelope {
            tx_version: TX_VERSION_1,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            tx_type: TxType::Transfer,
            sender,
            bootstrap_key: Some(keypair.key_descriptor()),
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

    /// A real, derivable sender address funded with `native_balance`
    /// directly in `store` -- the same bootstrap-key-must-derive-
    /// sender-address setup [`signed_transaction`]'s own `bootstrap_key`
    /// relies on.
    fn funded_sender(
        store: &mut InMemoryStateStore,
        seed: u8,
        native_balance: u128,
    ) -> NodeResult<(Digest, Ed25519KeyPair)> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32]);
        let sender = account_address_body(
            NETWORK_ID,
            keypair.key_descriptor().algorithm_id(),
            &keypair.key_descriptor().public_key_bytes(),
        )?;
        let key = account_section_state_key(&sender, AccountSection::Balance)?;
        store.set(key, BalanceValueV1 { native_balance }.encode())?;
        Ok((sender, keypair))
    }

    const PARENT: Digest = [0x30; 32];
    const PARENT_TIMESTAMP_MS: u64 = 1_000;
    const VALID_TIMESTAMP_MS: u64 = 2_000;

    #[test]
    fn accepts_a_legitimate_proposal_with_a_known_chain_position() -> NodeResult<()> {
        let fixture = fixture()?;
        let (envelope, proposer_signature) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        let expected_hash = envelope.block_hash()?;
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert_eq!(result, Ok(expected_hash));
        Ok(())
    }

    #[test]
    fn rejects_a_validator_that_is_not_this_rounds_legitimate_proposer() -> NodeResult<()> {
        let fixture = fixture()?;
        // Validator A's own real key, correctly self-signed -- a valid
        // signature from a real, registered validator who is simply
        // not the proposer this exact (height, round) selects.
        let impostor_id = [0x10_u8; 32];
        let impostor_keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, impostor_id);
        let (envelope, proposer_signature) = signed_envelope(
            &fixture.store,
            impostor_id,
            &impostor_keypair,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("legitimate proposer")));
        Ok(())
    }

    #[test]
    fn rejects_a_tampered_signature() -> NodeResult<()> {
        let fixture = fixture()?;
        let (envelope, mut proposer_signature) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        proposer_signature.signature[0] ^= 0xff;
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("signature invalid")));
        Ok(())
    }

    #[test]
    fn rejects_a_header_that_disagrees_with_its_own_body() -> NodeResult<()> {
        let fixture = fixture()?;
        let (mut envelope, _) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        // Tampered *after* the original signature was computed, then
        // re-signed over the tampered header's own (now different)
        // hash -- a real proposer could do exactly this; the
        // signature alone cannot catch a header that lies about its
        // own body.
        envelope.header.transactions_root = [0xaa; 32];
        let block_hash = envelope.block_hash()?;
        let proposer_signature = SignatureEnvelope {
            algorithm_id: fixture.legitimate_proposer.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: fixture.legitimate_proposer.sign(&block_hash).to_vec(),
        };
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("does not match header")));
        Ok(())
    }

    #[test]
    fn rejects_a_mismatched_state_root() -> NodeResult<()> {
        let fixture = fixture()?;
        let (mut envelope, _) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        // Tampered *after* the original signature was computed, then
        // re-signed over the tampered header's own (now different)
        // hash -- a real proposer could claim any `state_root` it
        // likes; nothing before this check in `verify_proposal` would
        // catch it, only the real recomputation this test exercises.
        envelope.header.state_root = [0xee; 32];
        let block_hash = envelope.block_hash()?;
        let proposer_signature = SignatureEnvelope {
            algorithm_id: fixture.legitimate_proposer.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: fixture.legitimate_proposer.sign(&block_hash).to_vec(),
        };
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("state_root")));
        Ok(())
    }

    #[test]
    fn rejects_a_wrong_parent_block_hash_when_chain_position_is_known() -> NodeResult<()> {
        let fixture = fixture()?;
        let (envelope, proposer_signature) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            [0xff; 32], // not PARENT
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("parent_block_hash")));
        Ok(())
    }

    #[test]
    fn rejects_a_non_monotonic_timestamp_when_chain_position_is_known() -> NodeResult<()> {
        let fixture = fixture()?;
        let (envelope, proposer_signature) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            PARENT_TIMESTAMP_MS, // not strictly after the parent's own
            Vec::new(),
        )?;
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("timestamp")));
        Ok(())
    }

    #[test]
    fn rejects_an_invalid_justification_quorum_certificate() -> NodeResult<()> {
        let fixture = fixture()?;
        let (mut envelope, proposer_signature) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        envelope.justification = Some(QuorumCertificate {
            certificate_type: VoteType::Prevote,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            epoch: Epoch::new(0),
            height: BlockHeight::new(HEIGHT),
            round: Round::new(ROUND),
            validator_set_commitment: DEVNET_VSC,
            target_type: VoteTargetType::Block,
            target_hash: envelope.block_hash()?,
            total_voting_power: 200,
            signed_voting_power: 200,
            signer_commitment: vec![0b0000_0011],
            // Fabricated, not a real signature from either registered
            // validator -- `verify_signatures` must catch this itself;
            // nothing upstream of it does.
            aggregate_proof: vec![
                SignatureEnvelope {
                    algorithm_id: 1,
                    key_reference: None,
                    signature: vec![0xaa; 64],
                },
                SignatureEnvelope {
                    algorithm_id: 1,
                    key_reference: None,
                    signature: vec![0xbb; 64],
                },
            ],
        });
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("justification")));
        Ok(())
    }

    #[test]
    fn accepts_a_legitimate_proposal_with_a_real_transaction() -> NodeResult<()> {
        let mut fixture = fixture()?;
        let (sender, keypair) = funded_sender(&mut fixture.store, 0x41, 1_000)?;
        let transaction = signed_transaction(&keypair, sender, 0)?;
        let (envelope, proposer_signature) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            vec![transaction],
        )?;
        let expected_hash = envelope.block_hash()?;
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert_eq!(result, Ok(expected_hash));
        Ok(())
    }

    #[test]
    fn rejects_receipts_that_do_not_match_real_execution() -> NodeResult<()> {
        let mut fixture = fixture()?;
        let (sender, keypair) = funded_sender(&mut fixture.store, 0x42, 1_000)?;
        let transaction = signed_transaction(&keypair, sender, 0)?;
        let (mut envelope, _) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            vec![transaction],
        )?;
        // Tampered *after* real execution already produced the
        // correct `Success` receipt -- recomputing `receipts_root`
        // from this lie keeps the header internally self-consistent
        // with the body (`verify_body_matches_header` alone cannot
        // catch this), but real re-execution inside `verify_proposal`
        // still produces the original, different receipt.
        envelope.body.receipts[0].status = ReceiptStatus::Failed;
        envelope.header.receipts_root = envelope.body.receipts_root()?;
        let block_hash = envelope.block_hash()?;
        let proposer_signature = SignatureEnvelope {
            algorithm_id: fixture.legitimate_proposer.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: fixture.legitimate_proposer.sign(&block_hash).to_vec(),
        };
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("receipts do not match")));
        Ok(())
    }

    #[test]
    fn rejects_a_transaction_that_would_abort_application() -> NodeResult<()> {
        let mut fixture = fixture()?;
        let (sender, keypair) = funded_sender(&mut fixture.store, 0x43, 1_000)?;
        // Nonce 5 while the account's real nonce is 0 (absent) --
        // `apply_block` must abort on this, so this block cannot be
        // built via `signed_envelope` (which itself needs real
        // application to succeed to derive real roots). Built by hand
        // instead: an internally self-consistent header/body pair
        // whose claimed receipt is pure fiction, since nothing can
        // honestly execute this transaction.
        let transaction = signed_transaction(&keypair, sender, 5)?;
        let tx_id = hn_state::tx_id(&transaction.encode()?)?;
        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions: vec![transaction],
            receipts: vec![ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Success,
            }],
            evidence: Vec::new(),
            extra_data: Vec::new(),
        };
        let header = BlockHeader {
            header_version: HEADER_VERSION_1,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            height: BlockHeight::new(HEIGHT),
            round: Round::new(ROUND),
            epoch: Epoch::new(0),
            protocol_epoch: ProtocolEpoch::GENESIS,
            parent_block_hash: PARENT,
            proposer: fixture.legitimate_proposer_id,
            timestamp: UnixTimeMillis::from_millis(VALID_TIMESTAMP_MS),
            transactions_root: body.transactions_root()?,
            // Never reached -- real application aborts before
            // `state_root` is ever checked.
            state_root: [0_u8; 32],
            receipts_root: body.receipts_root()?,
            events_root: list_empty_root()?,
            consensus_root: DEVNET_VSC,
            evidence_root: body.evidence_root()?,
            protocol_parameters_hash: protocol_parameters_placeholder_hash()?,
            extra_data_hash: extra_data_hash(&[])?,
        };
        let envelope = BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification: None,
        };
        let block_hash = envelope.block_hash()?;
        let proposer_signature = SignatureEnvelope {
            algorithm_id: fixture.legitimate_proposer.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: fixture.legitimate_proposer.sign(&block_hash).to_vec(),
        };
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let result = verify_proposal(
            &message,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            &fixture.store,
            &[],
            (PARENT, UnixTimeMillis::from_millis(PARENT_TIMESTAMP_MS)),
        );
        assert!(result.is_err_and(|reason| reason.contains("block application failed")));
        Ok(())
    }

    /// A real, verifiable `precommit` certificate for `block_hash`,
    /// signed by the one validator at `signer_bit_position` in
    /// `fixture.ordered_ids` -- every check
    /// `verify_and_commit_synced_block` can satisfy without external
    /// context already holds; callers tamper one field at a time to
    /// exercise one specific rejection.
    fn sample_commit_qc(
        block_hash: Digest,
        signer_id: Digest,
        keypair: &Ed25519KeyPair,
        signer_bit_position: usize,
    ) -> NodeResult<QuorumCertificate> {
        let payload = VoteSigningPayloadV1 {
            vote_type: VoteType::Precommit,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            epoch: Epoch::new(0),
            height: BlockHeight::new(HEIGHT),
            round: Round::new(ROUND),
            validator_set_commitment: DEVNET_VSC,
            validator_id: signer_id,
            target_type: VoteTargetType::Block,
            target_hash: block_hash,
            vote_metadata: Vec::new(),
        };
        let digest = payload.signing_digest()?;
        let signature = SignatureEnvelope {
            algorithm_id: keypair.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: keypair.sign(&digest).to_vec(),
        };
        Ok(QuorumCertificate {
            certificate_type: VoteType::Precommit,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            epoch: Epoch::new(0),
            height: BlockHeight::new(HEIGHT),
            round: Round::new(ROUND),
            validator_set_commitment: DEVNET_VSC,
            target_type: VoteTargetType::Block,
            target_hash: block_hash,
            total_voting_power: 100,
            signed_voting_power: 100,
            signer_commitment: vec![1_u8 << signer_bit_position],
            aggregate_proof: vec![signature],
        })
    }

    /// `fixture.legitimate_proposer_id`'s own position in
    /// `fixture.ordered_ids` (`[id_a, id_b]`, see `fixture`'s own doc
    /// comment) -- validator B, bit position 1.
    const SIGNER_BIT_POSITION: usize = 1;

    #[test]
    fn accepts_a_valid_synced_block() -> NodeResult<()> {
        let fixture = fixture()?;
        let (envelope, _) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        let block_hash = envelope.block_hash()?;
        let commit_qc = sample_commit_qc(
            block_hash,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            SIGNER_BIT_POSITION,
        )?;
        let block = FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope,
            commit_qc,
        };
        let mut store = fixture.store;
        let result = verify_and_commit_synced_block(
            &block,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            (BlockHeight::new(HEIGHT - 1), PARENT),
            &mut store,
            &[],
        );
        assert_eq!(result, Ok(block_hash));
        assert_eq!(
            store.finalized_block_range(BlockHeight::new(HEIGHT - 1), 10)?,
            vec![block]
        );
        Ok(())
    }

    #[test]
    fn rejects_a_forged_commit_qc() -> NodeResult<()> {
        let fixture = fixture()?;
        let (envelope, _) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        let block_hash = envelope.block_hash()?;
        let mut commit_qc = sample_commit_qc(
            block_hash,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            SIGNER_BIT_POSITION,
        )?;
        commit_qc.aggregate_proof[0].signature[0] ^= 0xff;
        let block = FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope,
            commit_qc,
        };
        let mut store = fixture.store;
        let result = verify_and_commit_synced_block(
            &block,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            (BlockHeight::new(HEIGHT - 1), PARENT),
            &mut store,
            &[],
        );
        assert!(result.is_err_and(|reason| reason.contains("commit_qc invalid")));
        Ok(())
    }

    #[test]
    fn rejects_a_height_that_skips_ahead() -> NodeResult<()> {
        let fixture = fixture()?;
        let (envelope, _) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        let block_hash = envelope.block_hash()?;
        let commit_qc = sample_commit_qc(
            block_hash,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            SIGNER_BIT_POSITION,
        )?;
        let block = FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope,
            commit_qc,
        };
        let mut store = fixture.store;
        // The envelope's own real height is `HEIGHT` (5); claiming
        // this node's own last accepted height is `HEIGHT - 2` (3)
        // skips height 4 entirely.
        let result = verify_and_commit_synced_block(
            &block,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            (BlockHeight::new(HEIGHT - 2), PARENT),
            &mut store,
            &[],
        );
        assert!(result.is_err_and(|reason| reason.contains("expected the next sequential height")));
        Ok(())
    }

    #[test]
    fn rejects_a_synced_block_with_a_mismatched_state_root() -> NodeResult<()> {
        let fixture = fixture()?;
        let (mut envelope, _) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        // Tampered *after* real execution already produced the
        // correct `state_root` -- `verify_body_matches_header` does
        // not check `state_root` at all (it is not derived from
        // `body`), so only the real re-execution this test exercises
        // can catch it.
        envelope.header.state_root = [0xee; 32];
        let block_hash = envelope.block_hash()?;
        let commit_qc = sample_commit_qc(
            block_hash,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            SIGNER_BIT_POSITION,
        )?;
        let block = FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope,
            commit_qc,
        };
        let mut store = fixture.store;
        let result = verify_and_commit_synced_block(
            &block,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            (BlockHeight::new(HEIGHT - 1), PARENT),
            &mut store,
            &[],
        );
        assert!(result.is_err_and(|reason| reason.contains("state_root")));
        Ok(())
    }

    #[test]
    fn rejects_a_block_from_a_foreign_chain() -> NodeResult<()> {
        let fixture = fixture()?;
        let (envelope, _) = signed_envelope(
            &fixture.store,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            PARENT,
            VALID_TIMESTAMP_MS,
            Vec::new(),
        )?;
        let block_hash = envelope.block_hash()?;
        let commit_qc = sample_commit_qc(
            block_hash,
            fixture.legitimate_proposer_id,
            &fixture.legitimate_proposer,
            SIGNER_BIT_POSITION,
        )?;
        let block = FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope,
            commit_qc,
        };
        let mut store = fixture.store;
        // This node's own real last-accepted block hash is not
        // `PARENT` -- the synced block does not attach to this node's
        // own chain at all.
        let result = verify_and_commit_synced_block(
            &block,
            CHAIN_ID,
            NETWORK_ID,
            &fixture.ordered_ids,
            (BlockHeight::new(HEIGHT - 1), [0xff; 32]),
            &mut store,
            &[],
        );
        assert!(result.is_err_and(|reason| reason.contains("foreign chain")));
        Ok(())
    }

    fn sample_genesis_manifest(genesis_time: u64, extra_data: Vec<u8>) -> GenesisManifest {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x01; 32]);
        GenesisManifest {
            manifest_version: crate::genesis::GENESIS_MANIFEST_VERSION_1,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            genesis_time,
            genesis_message: "test genesis".to_string(),
            validators: vec![GenesisValidator {
                validator_id: [0x01; 32],
                consensus_key: keypair.key_descriptor(),
                bonded_stake: hn_state::MINIMUM_VALIDATOR_BOND,
            }],
            allocations: GenesisAllocations {
                reserve: GenesisAccount {
                    address: [0xA0; 32],
                    amount: hn_state::RESERVE_ALLOCATION,
                },
                founder: GenesisAccount {
                    address: [0xA1; 32],
                    amount: hn_state::FOUNDER_ALLOCATION,
                },
                community: GenesisAccount {
                    address: [0xA2; 32],
                    amount: hn_state::COMMUNITY_ALLOCATION,
                },
            },
            extra_data,
        }
    }

    #[test]
    fn genesis_block_header_is_deterministic() -> NodeResult<()> {
        let manifest = sample_genesis_manifest(1_700_000_000, Vec::new());
        let first = genesis_block_header(&manifest)?;
        let second = genesis_block_header(&manifest)?;
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn genesis_block_header_has_no_predecessor_or_proposer() -> NodeResult<()> {
        let manifest = sample_genesis_manifest(1_700_000_000, Vec::new());
        let header = genesis_block_header(&manifest)?;
        assert_eq!(header.height, BlockHeight::GENESIS);
        assert_eq!(header.parent_block_hash, [0_u8; 32]);
        assert_eq!(header.proposer, [0_u8; 32]);
        Ok(())
    }

    #[test]
    fn genesis_block_header_converts_genesis_time_seconds_to_millis() -> NodeResult<()> {
        let manifest = sample_genesis_manifest(5, Vec::new());
        let header = genesis_block_header(&manifest)?;
        assert_eq!(header.timestamp.as_millis(), 5_000);
        Ok(())
    }

    #[test]
    fn genesis_block_header_commits_to_the_manifests_own_state_root() -> NodeResult<()> {
        let manifest = sample_genesis_manifest(1_700_000_000, Vec::new());
        let header = genesis_block_header(&manifest)?;
        assert_eq!(header.state_root, manifest.initial_state_root()?);
        Ok(())
    }

    #[test]
    fn genesis_block_header_commits_to_the_manifests_own_extra_data() -> NodeResult<()> {
        let manifest = sample_genesis_manifest(1_700_000_000, vec![0xab, 0xcd]);
        let header = genesis_block_header(&manifest)?;
        assert_eq!(
            header.extra_data_hash,
            extra_data_hash(&manifest.extra_data)?
        );
        // Not the always-empty hash every other (devnet, zero-
        // transaction) block currently commits to -- genesis's own
        // `extra_data` is real content here, and must actually affect
        // the hash, not be silently ignored by reusing the empty case.
        assert_ne!(header.extra_data_hash, extra_data_hash(&[])?);
        Ok(())
    }
}
