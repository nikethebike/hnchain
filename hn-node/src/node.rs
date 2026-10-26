use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use hn_consensus::{
    ConsensusAction, ConsensusEngine, ConsensusError, ConsensusStep, ConsensusTarget,
    JournalReadOutcome, JournalStartupAction, JournaledVote, VoteJournal, decide_startup_action,
    round_proposer, vote_to_send,
};
use hn_core::{BlockHeight, Epoch, ProtocolEpoch, ProtocolVersion, Round, UnixTimeMillis};
use hn_crypto::{Digest, Ed25519KeyPair, KeyRole, SignatureEnvelope, hash_profile_0x0001};
use hn_network::{
    Channel, ConsensusProposalMessageV1, HandshakeAction, HandshakeEvent, HandshakeParams,
    HandshakeState, Hello, MAX_PAYLOAD_LEN, MAX_SYNC_BLOCKS_PER_RESPONSE, MessageType,
    P2PMessageEnvelopeV1, PeerLink, SyncBlockRangeRequestV1, SyncBlockRangeResponseV1,
    TransactionAnnounceV1, TransactionRequestV1, TransactionResponseV1, spawn_peer_link,
};
use hn_rpc::{HttpResponse, ParsedRequest, RequestHandler, RpcError, ServerConfig, spawn_server};
use hn_state::{
    BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader, BlockStore,
    ConsensusVote, EmptyHashTable, FinalizedBlock, HEADER_VERSION_1, MAX_TRANSACTION_SIZE,
    ProposalRoundClaimV1, QuorumCertificate, ReceiptV1, StateCommitter, StateEnumerator,
    StateReader, ValidatorRecordV1, ValidatorStatus, VoteSigningPayloadV1, VoteTargetType,
    VoteType, Write, active_key, active_set, apply_block, compute_post_state_root, list_empty_root,
    protocol_parameters_placeholder_hash,
};
use hn_storage::{RedbStateStore, read_optional, write_atomically};

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

/// Dedicated non-zero exit code for ADR-0048 rule 3's fatal case: this
/// validator's own vote journal is for a height higher than the block
/// store's `latest_finalized + 1`, meaning the store has lost data this
/// validator already voted on. Fixed and documented here, per the
/// ADR's own "the value is fixed in implementation and documented" —
/// this process must never continue in any role once that is true.
const JOURNAL_INCONSISTENT_EXIT_CODE: i32 = 78;

/// A fresh journal for a validator that is starting (or resuming)
/// with no usable prior voting record to restore — matches exactly
/// what [`hn_consensus::ConsensusEngine::new_height`]'s own state
/// looks like, so the two always agree on what "no vote cast yet"
/// means.
fn fresh_journal(
    chain_id: u8,
    network_id: u16,
    validator_id: Digest,
    height: BlockHeight,
) -> VoteJournal {
    VoteJournal {
        chain_id,
        network_id,
        validator_id,
        height,
        round: Round::FIRST,
        step: ConsensusStep::NewHeight,
        locked_value: None,
        locked_round: None,
        valid_value: None,
        valid_round: None,
        valid_qc: None,
        last_prevote: None,
        last_precommit: None,
    }
}

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
    /// One RPC call to answer against this node's own current state
    /// (ADR-0046, "Decided: Architecture And Crate Direction") —
    /// constructed by [`RpcHandler::handle`], the real bridge from
    /// `hn_rpc::spawn_server`'s own worker threads into this loop,
    /// started from [`run`] whenever `config.rpc_listen` is `Some`.
    /// `reply` is a capacity-1 channel so sending never blocks the
    /// loop waiting on a slow or gone receiver; any deadline is the
    /// *caller's* own `recv_timeout` (ADR-0046's own `503 node_busy`
    /// on expiry), never this loop's concern.
    Rpc {
        call: hn_rpc::RpcCall,
        reply: mpsc::SyncSender<hn_rpc::RpcResult>,
    },
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
    /// Genesis block's own `block_hash` — fixed for this chain's whole
    /// lifetime, computed once at startup from the genesis manifest
    /// (ADR-0043). `RpcCall::Status`'s own `genesis_hash` field
    /// (ADR-0046) needs this distinct, never-changing value; nothing
    /// else in this file reads it, since every other check already
    /// has its own chain-position fields (`last_block_hash`, ...).
    genesis_hash: Digest,
    /// The vote journal file's own path (ADR-0048, "Decided: Vote
    /// Journal") — a sibling of `state.redb` inside this node's own
    /// `--data-dir`, not part of the block/state store itself.
    journal_path: PathBuf,
    /// This validator's own latest successfully-written journal
    /// record — the in-memory mirror of whatever
    /// [`Self::journal_path`] durably holds right now. [`cast_vote`]
    /// is the only place that ever changes it, and always writes the
    /// new value to disk first (ADR-0048 rule 1, "write before send").
    journal: VoteJournal,
    /// Set from [`hn_consensus::JournalStartupAction::AbstainThrough`]
    /// at startup (ADR-0048 rule 4): while `Some(height)`, this
    /// validator does not cast any vote and does not self-propose up
    /// through and including `height` — it still applies finalized
    /// blocks and syncs normally, exactly as if it held no voting
    /// power at all for that one height. Cleared the moment this
    /// engine's own height moves past it.
    abstain_until: Option<BlockHeight>,
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
    let latest_finalized_block = store.latest_finalized_block()?;
    let (starting_height, last_block_hash, last_timestamp) = match &latest_finalized_block {
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
    let latest_finalized_height = latest_finalized_block
        .as_ref()
        .map(|block| block.envelope.header.height);

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

    // ADR-0048, "Decided: Vote Journal (Write-Ahead Safety State)" --
    // rules 3 and 4, decided before this node ever casts a vote this
    // run. Rule 5 (genesis bootstrap vs. a validator rejoining with a
    // wiped data directory) is deferred (no peer-height-at-handshake
    // signal exists yet): an empty store with a missing/corrupt
    // journal falls back to voting normally, same as before this ADR,
    // a known and documented gap rather than a silent one (see
    // `hn_consensus::JournalStartupAction::Vote`'s own doc).
    let journal_path = config.data_dir.join("vote_journal");
    let journal_read = match read_optional(&journal_path)? {
        None => JournalReadOutcome::Missing,
        Some(bytes) => match VoteJournal::decode(&bytes) {
            Ok(journal) => JournalReadOutcome::Present(Box::new(journal)),
            Err(error) => JournalReadOutcome::Corrupt(error),
        },
    };
    let startup_action =
        decide_startup_action(journal_read, latest_finalized_height).map_err(|error| {
            NodeError::from(crate::config::ConfigError(format!(
                "vote journal startup decision failed: {error}"
            )))
        })?;
    let engine_height = BlockHeight::new(starting_height.get().saturating_add(1));
    let (engine, journal, abstain_until) = match startup_action {
        JournalStartupAction::Restore(journal) => {
            log_line(&format!(
                "RESTORED vote journal height={} round={} step={:?}",
                journal.height.get(),
                journal.round.get(),
                journal.step,
            ));
            (ConsensusEngine::restore(&journal), *journal, None)
        }
        JournalStartupAction::Vote => (
            ConsensusEngine::new_height(engine_height),
            fresh_journal(
                manifest.chain_id,
                manifest.network_id,
                own_validator_id,
                engine_height,
            ),
            None,
        ),
        JournalStartupAction::AbstainThrough { abstain_through } => {
            log_line(&format!(
                "ABSTAINING from voting through height={} (missing or corrupt vote journal)",
                abstain_through.get(),
            ));
            (
                ConsensusEngine::new_height(engine_height),
                fresh_journal(
                    manifest.chain_id,
                    manifest.network_id,
                    own_validator_id,
                    engine_height,
                ),
                Some(abstain_through),
            )
        }
        JournalStartupAction::Fatal {
            journal_height,
            store_height,
        } => {
            log_line(&format!(
                "FATAL: vote journal height={} exceeds this validator's own block store height={} + 1 \
                 -- the store has lost data this validator already voted on. Exiting with code {}.",
                journal_height.get(),
                store_height.get(),
                JOURNAL_INCONSISTENT_EXIT_CODE,
            ));
            std::process::exit(JOURNAL_INCONSISTENT_EXIT_CODE);
        }
    };

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
    let listener = TcpListener::bind(config.listen)?;
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

    // ADR-0046, "RPC v1": no RPC server at all unless this flag is
    // present (ADR-0038's own "every flag either required or has a
    // stated default" posture, extended -- `rpc_listen`'s own default
    // is "absent", not a magic address).
    if let Some(rpc_listen) = config.rpc_listen {
        let rpc_listener = TcpListener::bind(rpc_listen)?;
        let handler: Arc<dyn RequestHandler> = Arc::new(RpcHandler {
            event_tx: event_tx.clone(),
        });
        spawn_server(rpc_listener, rpc_server_config(), handler);
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
        // both a genesis start and a real restart alike -- unless the
        // vote journal just restored a different, more specific state
        // above.
        engine,
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
        genesis_hash: genesis_block_hash,
        journal_path,
        journal,
        abstain_until,
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
    listener: TcpListener,
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

/// The real first-ever driving step, called exactly once, the moment
/// enough peers are up (or the startup grace period elapses first).
/// The common case is a fresh engine at step `NewHeight`/`Timeout`,
/// where `begin_round()` is exactly right, same as it always was.
///
/// ADR-0048 rule 3's "restore" case means this engine can instead
/// already be mid-round (`Propose`/`Prevote`/`Precommit`/`Finalize`) --
/// calling `begin_round()` unconditionally here would hit
/// `ConsensusError::UnexpectedEvent` for any of those, since
/// `BeginRound` has no defined transition except from `NewHeight`/
/// `Timeout`. Found while writing a real restart-then-re-vote test:
/// a validator that restores mid-round and does *not* need historical
/// sync (the common, intended restore case: its peers are still on
/// the exact same round) would otherwise crash on its own very first
/// peer connection instead of resuming.
///
/// Resuming at `Prevote`/`Precommit` re-sends this validator's own
/// already-journaled vote for the current round (ADR-0048 rule 2:
/// "the node may re-send exactly that vote" -- not only a defensive
/// backstop for a hypothetical double-decision bug, but the real
/// recovery for rule 1's own named "a crash between write and send
/// loses a vote" window, where the broadcast genuinely never
/// happened the first time). `cast_vote` is called with a throwaway
/// `Nil` target rather than a freshly decided one: `vote_to_send`
/// inside it always prefers whatever the journal already holds for
/// this exact `(height, round)`, which this restored engine's own
/// `step` guarantees is present and matches (a journal write and its
/// matching step transition happen together, atomically, in the same
/// `cast_vote` call that produced them) -- so the placeholder is
/// always overridden, never actually sent.
///
/// Resuming at `Propose` just re-arms its own timeout: this process's
/// vote journal names no proposal content at all, only votes, so
/// silently rebuilding a new block here risks signing a *second*,
/// different proposal for a (height, round) this validator may
/// already have proposed for before it died (ADR-0048, "Decided:
/// Proposer Duties": "a proposer may not send two different proposals
/// for the same (height, round)"). `Finalize` resumes nothing:
/// committing needs the real envelope this process no longer holds in
/// memory, and this height will finalize this validator's own way
/// regardless, through a later observed height (sync) or a fresh
/// precommit certificate re-delivery.
fn resume_or_begin_round(ctx: &mut Ctx) -> NodeResult<()> {
    let height = ctx.engine.state().height;
    let round = ctx.engine.state().round;
    match ctx.engine.state().step {
        ConsensusStep::NewHeight | ConsensusStep::Timeout => {
            let action = ctx.engine.begin_round()?;
            act_on(ctx, action)
        }
        ConsensusStep::Propose => {
            spawn_after(
                ctx.event_tx.clone(),
                stage_timeout(ctx.base_timeout_ms, round),
                NodeEvent::ProposeTimeout {
                    height: height.get(),
                    round: round.get(),
                },
            );
            Ok(())
        }
        ConsensusStep::Prevote => {
            if let Some(action) = cast_vote(ctx, VoteType::Prevote, ConsensusTarget::Nil)? {
                act_on(ctx, action)?;
            }
            Ok(())
        }
        ConsensusStep::Precommit => {
            if let Some(action) = cast_vote(ctx, VoteType::Precommit, ConsensusTarget::Nil)? {
                act_on(ctx, action)?;
            }
            Ok(())
        }
        ConsensusStep::Finalize => Ok(()),
    }
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
                    resume_or_begin_round(ctx)?;
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
                if ctx.syncing.is_none() && ctx.engine.state().height.get() == height {
                    let action = ctx.engine.propose_timeout(Round::new(round))?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::PrevoteTimeout { height, round } => {
                if ctx.syncing.is_none() && ctx.engine.state().height.get() == height {
                    let action = ctx.engine.prevote_timeout(Round::new(round))?;
                    act_on(ctx, action)?;
                }
            }
            NodeEvent::PrecommitTimeout { height, round } => {
                if ctx.syncing.is_none() && ctx.engine.state().height.get() == height {
                    let action = ctx.engine.precommit_timeout(Round::new(round))?;
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
                    resume_or_begin_round(ctx)?;
                }
            }
            NodeEvent::Rpc { call, reply } => {
                let result = dispatch_rpc_call(ctx, call)?;
                // ADR-0046, "Decided: Transaction Submission": "the
                // transaction is announced to peers exactly as a
                // P2P-admitted one" -- the same re-broadcast the
                // `TransactionResponse` handler above already does,
                // since an RPC submission reuses the exact same
                // `Mempool::admit` pipeline and needs the exact same
                // propagation.
                if let Ok(hn_rpc::RpcReply::Submitted(submitted)) = &result {
                    broadcast(
                        ctx,
                        Channel::Transactions,
                        MessageType::TransactionAnnounce,
                        TransactionAnnounceV1 {
                            tx_id: submitted.tx_id,
                        }
                        .encode(),
                    )?;
                }
                // A receiver that already gave up (its own
                // `recv_timeout` deadline, ADR-0046) is not this loop's
                // problem -- the work was still done at the cost of
                // one bounded call, and the loop moves on either way.
                let _ = reply.send(result);
            }
        }
    }
    Ok(())
}

/// Services one [`hn_rpc::RpcCall`] against `ctx`'s own current state
/// (ADR-0046, "Decided: Architecture And Crate Direction") — builds the
/// narrow [`crate::rpc_bridge::RpcContext`] view
/// [`crate::rpc_bridge::handle_rpc_call`] actually needs from the full
/// node `Ctx`, the same boundary every other free function in this
/// file already draws against it.
fn dispatch_rpc_call(ctx: &mut Ctx, call: hn_rpc::RpcCall) -> NodeResult<hn_rpc::RpcResult> {
    let mut rpc_ctx = crate::rpc_bridge::RpcContext {
        store: &ctx.store,
        mempool: &mut ctx.mempool,
        chain_id: ctx.chain_id,
        network_id: ctx.network_id,
        genesis_hash: ctx.genesis_hash,
        protocol_version: (
            PROTOCOL_VERSION.major(),
            PROTOCOL_VERSION.minor(),
            PROTOCOL_VERSION.patch(),
        ),
        last_finalized_height: ctx.last_finalized_height,
        last_block_hash: ctx.last_block_hash,
        last_timestamp: ctx.last_timestamp,
        validator_candidates: &ctx.records,
        peer_count: ctx.established.len(),
        syncing: ctx.syncing.is_some(),
    };
    crate::rpc_bridge::handle_rpc_call(call, &mut rpc_ctx)
}

/// The devnet default for how long an RPC worker thread waits for the
/// event loop's own answer before giving up (ADR-0046, "Decided:
/// Architecture And Crate Direction" — "the devnet default 2 s").
const RPC_REPLY_DEADLINE: Duration = Duration::from_secs(2);

/// [`hn_rpc::spawn_server`]'s own devnet worker-pool/deadline profile
/// (ADR-0046's own "implementation constants, documented" —
/// documented here, the one place this crate starts that server).
/// `max_body_len` is exactly ADR-0046's own named bound: `2 *
/// MAX_TRANSACTION_SIZE + 1 KiB` (hex doubling a canonical
/// transaction's own bytes, plus a small JSON envelope).
fn rpc_server_config() -> ServerConfig {
    ServerConfig {
        worker_count: 4,
        accept_queue_capacity: 16,
        total_read_deadline: Duration::from_secs(5),
        per_read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(5),
        max_body_len: MAX_TRANSACTION_SIZE.saturating_mul(2).saturating_add(1024),
    }
}

/// [`hn_rpc::RequestHandler`] for this node's own RPC server — routes
/// the request, submits a successfully-routed call into the running
/// event loop as a [`NodeEvent::Rpc`], and waits up to
/// [`RPC_REPLY_DEADLINE`] for its answer (ADR-0046, "Decided:
/// Architecture And Crate Direction": "If the loop does not answer
/// within a deadline ... the server returns `503 node_busy`. Slow RPC
/// must degrade RPC, never consensus" — this struct is the only thing
/// that ever blocks on that wait; the event loop itself never waits on
/// this struct for anything).
struct RpcHandler {
    event_tx: Sender<NodeEvent>,
}

impl RequestHandler for RpcHandler {
    fn handle(&self, request: ParsedRequest<'_>) -> HttpResponse {
        let call = match hn_rpc::route(&request) {
            Ok(call) => call,
            Err(error) => return hn_rpc::error_response(error.status_code(), error.code()),
        };

        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        if self
            .event_tx
            .send(NodeEvent::Rpc {
                call,
                reply: reply_tx,
            })
            .is_err()
        {
            return node_busy_response();
        }
        match reply_rx.recv_timeout(RPC_REPLY_DEADLINE) {
            Ok(Ok(reply)) => hn_rpc::reply_to_response(&reply),
            Ok(Err(error)) => hn_rpc::error_response(error.status_code(), error.code()),
            Err(_) => node_busy_response(),
        }
    }
}

fn node_busy_response() -> HttpResponse {
    hn_rpc::error_response(RpcError::NodeBusy.status_code(), RpcError::NodeBusy.code())
}

// `is_current`/`expects_qc` (the old height+round+step caller-side
// filter) are gone (ADR-0048, "Decided: Certificate Routing In
// `hn-node`"): every timeout/vote-quorum/proposal handler below now
// checks only height before calling into `ctx.engine`, and passes its
// own real round straight through. Round and step scoping — including
// the exact same-height/round, different-step staleness this file's
// own history found this way (a self-proposing node that casts its
// own prevote synchronously, before its just-scheduled `ProposeTimeout`
// ever fires, leaving that timer stale for a step the engine already
// left) — is the machine's own job now: every `ConsensusEngine` method
// below returns `ConsensusAction::Ignored` for a stale round and/or
// step rather than `ConsensusError::UnexpectedEvent`, since each one
// was changed (ADR-0048, phases 1 and 4) to carry its own round instead
// of silently substituting the engine's current one.

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
                if qc_height_matches(ctx, &qc) {
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
            if qc_height_matches(ctx, &qc) {
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
            // ADR-0048, "Decided: Certificate Routing In `hn-node`":
            // only height is filtered here. A proposal for a round the
            // engine already left behind, or has not reached yet, is
            // still fully verified below and handed to
            // `ConsensusEngine::handle_proposal`, which owns round/step
            // scoping itself (R2, R3) and returns `Ignored` for one
            // that is not for its own current round/step -- but still
            // records it as a *validated* proposal either way (R8's
            // own "any round" decide needs exactly this, found while
            // wiring this: an old-round proposal this validator
            // genuinely verified must not be forgotten just because it
            // did not arrive while the engine was still waiting on it).
            if header.height.get() == ctx.engine.state().height.get() {
                // Receiver-side verification (ADR-0042, "Decided:
                // Receiver-Side Verification Rules") -- a failure here
                // is a dropped, logged message, never a fatal error:
                // this is externally-triggerable network input, and no
                // externally-triggerable input may crash this process.
                // `round_proposer` inside `verify_proposal` checks
                // eligibility against `message.for_round` (ADR-0049),
                // so this is correct for any round of the current
                // height, not just the engine's own current one.
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
                        let valid_round =
                            message.envelope.justification.as_ref().map(|qc| qc.round);
                        let action = ctx.engine.handle_proposal(
                            message.for_round,
                            block_hash,
                            message.envelope.body.transactions,
                            valid_round,
                            message.envelope.justification,
                            true,
                        )?;
                        act_on(ctx, action)?;
                        // R8's "need block" half: a precommit quorum
                        // for this exact block may already be pending
                        // (received before this validator ever saw a
                        // validated proposal for it) -- safe to call
                        // unconditionally, a no-op `Ignored` when
                        // nothing is pending.
                        let retry_action = ctx.engine.block_fetched(block_hash)?;
                        act_on(ctx, retry_action)?;
                    }
                    Err(reason) => {
                        log_line(&format!(
                            "REJECTED proposal height={} round={} reason=\"{reason}\"",
                            header.height.get(),
                            message.for_round.get(),
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
            // ADR-0044's own named "Unbounded response size" security
            // consideration is only half closed by
            // `MAX_SYNC_BLOCKS_PER_RESPONSE`: that bounds how many
            // blocks, not how big they are, and a real finalized
            // block's own size varies hugely (an empty block vs. one
            // packed with `MAX_TRANSACTIONS_PER_BLOCK` transactions).
            // Trimming here, not inside `BlockStore::finalized_block_range`
            // itself, keeps that trait's own contract purely
            // height-based and puts the wire-size policy where the
            // wire-size constant (`MAX_PAYLOAD_LEN`) already lives.
            let blocks = fit_blocks_within_byte_budget(blocks, SYNC_RESPONSE_BYTE_BUDGET)?;
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

/// Maximum total bytes one `SyncBlockRangeResponseV1` may occupy once
/// wrapped in a `P2PMessageEnvelopeV1` (ADR-0044's own named
/// "Unbounded response size" security consideration) — generous
/// margin below `hn_network::MAX_PAYLOAD_LEN` for that envelope's own
/// small fixed overhead (version, chain/network id, channel, message
/// type, message id), so this response itself can never be the thing
/// that turns `send_to`'s own `.encode()` call into the real error
/// `MAX_SYNC_BLOCKS_PER_RESPONSE` alone cannot prevent: that bounds
/// how many blocks, not how big they are, and a real finalized
/// block's own size varies hugely (an empty block vs. one packed with
/// `MAX_TRANSACTIONS_PER_BLOCK` transactions).
const SYNC_RESPONSE_BYTE_BUDGET: usize = MAX_PAYLOAD_LEN - 4096;

/// Trims `blocks` (already in ascending, contiguous height order)
/// down to however many fit within `byte_budget` once the whole
/// response is encoded — a prefix, never a block skipped in the
/// middle (ADR-0044's own "never a gap-containing or reordered"
/// rule: once one block does not fit, nothing after it could either,
/// since height order is fixed). Always keeps at least the first
/// block, even if it alone exceeds `byte_budget` — a lagging node
/// must keep making real progress one block at a time rather than be
/// offered nothing at all because the next real block happens to be
/// large.
fn fit_blocks_within_byte_budget(
    blocks: Vec<FinalizedBlock>,
    byte_budget: usize,
) -> NodeResult<Vec<FinalizedBlock>> {
    let mut selected: Vec<FinalizedBlock> = Vec::new();
    for block in blocks {
        let is_first = selected.is_empty();
        selected.push(block);
        let encoded_len = SyncBlockRangeResponseV1 {
            blocks: selected.clone(),
        }
        .encode()?
        .len();
        if encoded_len > byte_budget {
            if !is_first {
                selected.pop();
            }
            break;
        }
    }
    Ok(selected)
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
/// claim), chain/network binding, proposer signature — resolved and
/// verified against `round_proposer(height, message.for_round)`'s own
/// registered consensus key (ADR-0049), **not** `envelope.header.
/// proposer`: `for_round` is the round this delivery actually claims
/// to be for, and for a re-proposal (`for_round` later than `header.
/// round`) `header.proposer` legitimately names whichever validator
/// first built the value, not whoever is re-delivering it now. The
/// signature verifies over `message.round_claim()`'s own signing
/// digest (binding `for_round` into what is actually signed, not just
/// checked structurally, is what keeps a re-proposal's own round
/// claim from being forgeable by a relay — see `ProposalRoundClaimV1`'s
/// own doc) — there is no separate `header.proposer` identity
/// comparison, since this signature check already ties the real
/// sender to the real current round's own proposer on its own.
/// Header-vs-body consistency
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

    // ADR-0049: who must have sent *this delivery* is
    // `round_proposer(height, for_round)` -- the round this message
    // actually claims to be for, not `header.round` (which, for a
    // re-proposal, still names whatever round the value was
    // *originally* built for and cannot change without changing
    // `block_hash`). `header.proposer` is not checked against this at
    // all: for a re-proposal it legitimately names a *different*
    // validator (whoever first built the value this one is now
    // vouching for at a later round), and the signature check below
    // already ties this specific delivery to the real current
    // round's own proposer regardless of what `header.proposer`
    // claims -- there is nothing left for a separate identity
    // comparison to protect that the signature check does not already
    // cover.
    let round_sender = round_proposer(ordered_ids, header.height, message.for_round)
        .ok_or_else(|| "no proposer exists for this round".to_string())?;
    let key = active_key(store, &round_sender)
        .map_err(|error| format!("could not resolve the proposer's registered key: {error}"))?
        .ok_or_else(|| "the proposer has no registered consensus key".to_string())?;
    let round_claim = message
        .round_claim()
        .map_err(|error| format!("could not build the round claim: {error}"))?;
    let round_claim_digest = round_claim
        .signing_digest()
        .map_err(|error| format!("could not compute the round claim digest: {error}"))?;
    message
        .proposer_signature
        .verify(&key, &round_claim_digest)
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

/// ADR-0048, "Decided: Certificate Routing In `hn-node`" item 1: `qc`
/// is dropped here only if its height differs from this engine's
/// current height (or it is malformed, checked separately via
/// `verify_signatures`). Every other certificate — any round of this
/// same height, including one the engine already moved past — is
/// passed straight to [`hn_consensus::ConsensusEngine::handle_quorum_certificate`],
/// which owns all round and step scoping itself (R5, R6, R8, R9, R10)
/// and returns `Ignored` rather than an error for a redundant or
/// superseded one (e.g. two honest nodes each independently
/// aggregating their own certificate from the same gossiped votes,
/// ADR-0037's "no designated aggregator," routinely produces more
/// than one certificate for the same target).
fn qc_height_matches(ctx: &Ctx, qc: &QuorumCertificate) -> bool {
    ctx.engine.state().height == qc.height
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

/// Drives `action`, and every follow-up action it produces, to
/// completion — the *only* place in this file that may ever call
/// itself again for a new action; [`act_on_once`],
/// [`maybe_start_propose_stage`], and [`cast_vote`] each return their
/// own follow-up action (if any) rather than ever calling [`act_on`]
/// a second time.
///
/// That matters because a real chain of actions can run many steps
/// deep with no network round-trip, or even a return to this crate's
/// own event loop, in between: self-propose (`None` ->
/// `maybe_start_propose_stage`) -> self-prevote (`Prevote` ->
/// `cast_vote`) -> self-precommit (`Precommit` -> `cast_vote` again)
/// -> finalize (`Finalized`) -> the *next* height's own `NewHeight` ->
/// `None` again — every step of which a validator holding enough
/// voting power to reach quorum with itself alone (the degenerate but
/// entirely legitimate single-validator devnet case) completes on its
/// own, for every height, forever. Each of those five functions used
/// to call `act_on` again directly for its own next action, so this
/// whole chain recursed through all of them once per height with no
/// bound at all, and a long enough run overflowed the stack (found
/// running exactly this scenario for a few seconds, ADR-0046's own
/// `rpc_listen.rs`/`rpc_end_to_end.rs` test development, both of which
/// sidestep it by using 2+ validators instead). This loop is the
/// trampoline instead: whatever `act_on_once` (transitively including
/// `maybe_start_propose_stage`/`cast_vote`) returns becomes the next
/// iteration's own input, so a thousand cascading heights cost this
/// one stack frame exactly once, never one per height.
fn act_on(ctx: &mut Ctx, action: ConsensusAction) -> NodeResult<()> {
    let mut next = Some(action);
    while let Some(action) = next {
        next = act_on_once(ctx, action)?;
    }
    Ok(())
}

/// One [`ConsensusAction`]'s own real handling — [`act_on`]'s own
/// internal step, returning whatever follow-up action it (or, for
/// `None`/`Prevote`/`Precommit`, the helper it delegates to) produced
/// instead of ever calling [`act_on`] again, so its own trampoline
/// loop can drive an arbitrarily long cascade of them in constant
/// stack space.
fn act_on_once(ctx: &mut Ctx, action: ConsensusAction) -> NodeResult<Option<ConsensusAction>> {
    Ok(match action {
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
                return Ok(None);
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
                    Some(ctx.engine.begin_new_height()?)
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
                    None
                }
                Err(error) => return Err(error.into()),
            }
        }
        ConsensusAction::RoundAdvanced { .. } | ConsensusAction::NewHeight { .. } => {
            Some(ctx.engine.begin_round()?)
        }
        // R8's "need block" half (ADR-0048): a precommit quorum
        // already proves `block_hash` final, but this validator has
        // no validated proposal for it yet. Nothing to do here but
        // record the wait in the log -- the engine itself already
        // remembers the pending certificate internally, and the
        // proposal branch of `handle_peer_message` retries it via
        // `ConsensusEngine::block_fetched` the moment a matching
        // proposal is validated (by any means: ordinary gossip is
        // enough, since the engine's own round scoping now accepts a
        // validated proposal for any round of this height, not only
        // the one currently being contested).
        ConsensusAction::NeedBlock { block_hash, .. } => {
            log_line(&format!(
                "WAITING_FOR_BLOCK height={} round={} block={} (precommit quorum certified, no cached proposal yet)",
                ctx.engine.state().height.get(),
                ctx.engine.state().round.get(),
                hex(&block_hash),
            ));
            None
        }
        // Well-formed but redundant (ADR-0048, "Decided: Certificate
        // Routing In `hn-node`" item 3) -- nothing for this driving
        // loop to do, same as `ConsensusAction::None`'s own "no vote to
        // cast" case, just for a different reason.
        ConsensusAction::Ignored { .. } => None,
    })
}

/// If this node is this round's own proposer, self-proposes and
/// returns the engine's own resulting action (its own `handle_proposal`
/// call's return value) for [`act_on`]'s trampoline to drive next --
/// never calling [`act_on`] itself here, unlike before: a proposer
/// that also holds enough voting power to reach quorum on its own
/// (the single-validator devnet case) would otherwise cascade through
/// propose/prevote/precommit/finalize *and the next height's own
/// propose* one native call deeper each time, with no bound (see
/// [`act_on`]'s own doc comment).
fn maybe_start_propose_stage(ctx: &mut Ctx) -> NodeResult<Option<ConsensusAction>> {
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

    // ADR-0048 rule 4: "it ... may propose only from the resumed
    // height" -- an abstaining validator still schedules its own
    // propose timeout above (so its local round/step bookkeeping
    // keeps advancing normally off real network traffic), it just
    // never acts as proposer while abstaining.
    let abstaining = ctx
        .abstain_until
        .is_some_and(|through| height.get() <= through.get());
    if !abstaining && round_proposer(&ctx.ordered_ids, height, round) == Some(ctx.own_validator_id)
    {
        return Ok(Some(propose_for_round(ctx, height, round)?));
    }
    Ok(None)
}

/// This round's own proposal content, once this validator has
/// already decided it is this round's proposer: re-proposes
/// `valid_value` with its own polka (ADR-0048 R1; ADR-0049 wires the
/// wire-level mechanics this needs) when this engine holds one *and*
/// still has that exact value's own envelope cached, falling back to
/// building a fresh block otherwise -- including when `valid_value`
/// is `Some` but its envelope was lost (a restarted validator's vote
/// journal, ADR-0048 rule 1, restores `valid_value`/`valid_qc` but
/// names no proposal content at all to repeat; `resume_or_begin_round`
/// already makes the same documented trade-off for the
/// restored-at-`Propose` case). Building fresh here never conflicts
/// with "a proposer may not send two different proposals for the same
/// (height, round)" (ADR-0048, "Decided: Proposer Duties"): nothing
/// has been sent for *this* round yet either way.
fn propose_for_round(
    ctx: &mut Ctx,
    height: BlockHeight,
    round: Round,
) -> NodeResult<ConsensusAction> {
    let valid_value = ctx.engine.state().valid_value;
    let valid_qc = ctx.engine.state().valid_qc.clone();
    if let (Some(value_hash), Some(valid_qc)) = (valid_value, valid_qc)
        && let Some(cached_envelope) = ctx.pending_envelopes.get(&value_hash).cloned()
    {
        return re_propose_valid_value(ctx, round, cached_envelope, valid_qc);
    }
    propose_fresh_block(ctx, height, round)
}

/// ADR-0049, "Decided: Proposer-Side Re-Proposal Logic": resends
/// `envelope` with its own `header`/`body` completely unchanged
/// (`block_hash` must stay exactly what `valid_qc` already certifies)
/// except `justification`, updated to this engine's own *current*
/// `valid_qc` -- not necessarily whatever `envelope` already carried
/// when it was first cached, since a value can become `valid_value`
/// through a polka that formed *after* it was originally proposed
/// (`justification` is not part of `block_hash`, so changing it here
/// is safe: `BlockEnvelope::block_hash` delegates to `header.
/// block_hash` alone, and `verify_body_matches_header` never looks at
/// `justification` either).
fn re_propose_valid_value(
    ctx: &mut Ctx,
    for_round: Round,
    mut envelope: BlockEnvelope,
    valid_qc: QuorumCertificate,
) -> NodeResult<ConsensusAction> {
    envelope.justification = Some(valid_qc.clone());
    let block_hash = envelope.block_hash()?;
    let transactions_for_engine = envelope.body.transactions.clone();
    ctx.pending_envelopes.insert(block_hash, envelope.clone());

    let round_claim = ProposalRoundClaimV1 {
        chain_id: envelope.header.chain_id,
        network_id: envelope.header.network_id,
        for_round,
        block_hash,
    };
    let proposer_signature = SignatureEnvelope {
        algorithm_id: ctx.consensus_keypair.key_descriptor().algorithm_id(),
        key_reference: None,
        signature: ctx
            .consensus_keypair
            .sign(&round_claim.signing_digest()?)
            .to_vec(),
    };
    broadcast(
        ctx,
        Channel::Consensus,
        MessageType::ConsensusProposal,
        ConsensusProposalMessageV1 {
            envelope,
            for_round,
            proposer_signature,
        }
        .encode()?,
    )?;
    let valid_round = valid_qc.round;
    Ok(ctx.engine.handle_proposal(
        for_round,
        block_hash,
        transactions_for_engine,
        Some(valid_round),
        Some(valid_qc),
        true,
    )?)
}

/// The pre-ADR-0049 path, unchanged in substance: build a brand new
/// block from this node's own mempool and propose it fresh, with no
/// `justification` (nothing has been validated/locked yet to justify
/// at this round).
fn propose_fresh_block(
    ctx: &mut Ctx,
    height: BlockHeight,
    round: Round,
) -> NodeResult<ConsensusAction> {
    // ADR-0045, "Decided: Block Building (Proposer)" -- select from
    // this node's own mempool, then actually run the result through
    // `apply_block` for real (not a second, separate simulation):
    // `select_for_block` already ran this exact check per candidate,
    // but re-deriving `result` from the final selection here, rather
    // than threading it out of `select_for_block` itself, keeps "what
    // got selected" and "what the real receipts/roots are" from two
    // different internal code paths that could silently drift apart.
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
    // `commit_finalized_block`, once this block actually finalizes)
    // needs its own, separate copy of exactly the same transactions.
    let transactions_for_engine = body.transactions.clone();
    let envelope = BlockEnvelope {
        block_version: BLOCK_VERSION_1,
        header,
        body,
        justification: None,
    };
    ctx.pending_envelopes.insert(block_hash, envelope.clone());
    let round_claim = ProposalRoundClaimV1 {
        chain_id: envelope.header.chain_id,
        network_id: envelope.header.network_id,
        for_round: round,
        block_hash,
    };
    let proposer_signature = SignatureEnvelope {
        algorithm_id: ctx.consensus_keypair.key_descriptor().algorithm_id(),
        key_reference: None,
        signature: ctx
            .consensus_keypair
            .sign(&round_claim.signing_digest()?)
            .to_vec(),
    };
    broadcast(
        ctx,
        Channel::Consensus,
        MessageType::ConsensusProposal,
        ConsensusProposalMessageV1 {
            envelope,
            for_round: round,
            proposer_signature,
        }
        .encode()?,
    )?;
    Ok(ctx
        .engine
        .handle_proposal(round, block_hash, transactions_for_engine, None, None, true)?)
}

/// Casts and broadcasts `vote_type`, then — if this node's own vote
/// was itself enough to form a real quorum certificate (ADR-0037's
/// own devnet case: a validator holding enough voting power to reach
/// quorum with itself alone) — returns the engine's own resulting
/// action for [`act_on`]'s trampoline to drive next, the same
/// never-call-`act_on`-directly discipline [`maybe_start_propose_stage`]
/// now also follows.
///
/// ADR-0048 rules 1 and 2: `target` is first replaced with whatever
/// this validator already journaled for this exact `(height, round,
/// vote_type)`, if anything — protecting against ever signing a
/// *different* vote than one already sent, including across a crash
/// and restart. The journal reflecting that final target and this
/// engine's own current lock/valid state is then written durably
/// *before* anything is signed or sent ("the machine never signs;
/// signing happens in `hn-node` after the vote journal write" --
/// ADR-0048, "Decided: Boundary").
fn cast_vote(
    ctx: &mut Ctx,
    vote_type: VoteType,
    target: ConsensusTarget,
) -> NodeResult<Option<ConsensusAction>> {
    let height = ctx.engine.state().height;
    let round = ctx.engine.state().round;

    // ADR-0048 rule 4: abstain through the one height a missing or
    // corrupt journal left this validator unsure what it already
    // signed -- no vote, no journal write, nothing sent.
    if let Some(abstain_through) = ctx.abstain_until {
        if height.get() <= abstain_through.get() {
            return Ok(None);
        }
        ctx.abstain_until = None;
    }

    let already_journaled = match vote_type {
        VoteType::Prevote => ctx.journal.last_prevote.as_ref(),
        VoteType::Precommit => ctx.journal.last_precommit.as_ref(),
    };
    let target = vote_to_send(already_journaled, height, round, target);
    let (target_type, target_hash) = match target {
        ConsensusTarget::Block(hash) => (VoteTargetType::Block, hash),
        ConsensusTarget::Nil => (VoteTargetType::Nil, [0_u8; 32]),
    };

    let journaled_vote = JournaledVote {
        height,
        round,
        target,
    };
    let mut new_journal = ctx.journal.clone();
    new_journal.height = height;
    new_journal.round = round;
    new_journal.step = ctx.engine.state().step;
    new_journal.locked_value = ctx.engine.state().locked_value;
    new_journal.locked_round = ctx.engine.state().locked_round;
    new_journal.valid_value = ctx.engine.state().valid_value;
    new_journal.valid_round = ctx.engine.state().valid_round;
    new_journal.valid_qc = ctx.engine.state().valid_qc.clone();
    match vote_type {
        VoteType::Prevote => new_journal.last_prevote = Some(journaled_vote),
        VoteType::Precommit => new_journal.last_precommit = Some(journaled_vote),
    }
    let encoded_journal = new_journal.encode().map_err(NodeError::from)?;
    write_atomically(&ctx.journal_path, &encoded_journal)?;
    ctx.journal = new_journal;

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
    // ADR-0048's own Verification Requirement 4: "the node logs every
    // signed vote; the test compares them" -- a real-process restart
    // test's only way to check this validator never signs a different
    // vote for a (height, round, vote_type) it already journaled.
    log_line(&format!(
        "VOTED height={} round={} type={vote_type:?} target={}",
        height.get(),
        round.get(),
        match target {
            ConsensusTarget::Block(hash) => hex(&hash),
            ConsensusTarget::Nil => "NIL".to_string(),
        },
    ));

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
        if qc_height_matches(ctx, &qc) {
            let action = ctx
                .engine
                .handle_quorum_certificate(qc, &ctx.store, &ctx.ordered_ids)?;
            return Ok(Some(action));
        }
    }
    Ok(None)
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
        AccessListV1, AccountSection, BalanceValueV1, FINALIZED_BLOCK_VERSION_1,
        MAX_EVIDENCE_BLOB_LEN, ReceiptStatus, StateWriter, TX_VERSION_1, TransactionEnvelope,
        TransferPayloadV1, TxType, ValidatorSection, ValidityWindowV1, account_section_state_key,
        extra_data_hash, validator_section_state_key,
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
        let round_claim = ProposalRoundClaimV1 {
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            for_round: Round::new(ROUND),
            block_hash,
        };
        let signature = SignatureEnvelope {
            algorithm_id: keypair.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: keypair.sign(&round_claim.signing_digest()?).to_vec(),
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
            for_round: envelope.header.round,
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
        // A real, self-consistent signature from a validator who is simply
        // not the proposer this exact (height, round) selects. ADR-0049:
        // `verify_proposal` resolves the expected signer purely from
        // (height, for_round) via `round_proposer`, then checks the
        // signature against *that* validator's key -- so a different
        // validator's otherwise-valid signature is rejected as a signature
        // mismatch, the same path a tampered signature takes.
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
            for_round: envelope.header.round,
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
            for_round: envelope.header.round,
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
        let round_claim = ProposalRoundClaimV1 {
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            for_round: envelope.header.round,
            block_hash,
        };
        let proposer_signature = SignatureEnvelope {
            algorithm_id: fixture.legitimate_proposer.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: fixture
                .legitimate_proposer
                .sign(&round_claim.signing_digest()?)
                .to_vec(),
        };
        let message = ConsensusProposalMessageV1 {
            for_round: envelope.header.round,
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
        let round_claim = ProposalRoundClaimV1 {
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            for_round: envelope.header.round,
            block_hash,
        };
        let proposer_signature = SignatureEnvelope {
            algorithm_id: fixture.legitimate_proposer.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: fixture
                .legitimate_proposer
                .sign(&round_claim.signing_digest()?)
                .to_vec(),
        };
        let message = ConsensusProposalMessageV1 {
            for_round: envelope.header.round,
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
            for_round: envelope.header.round,
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
            for_round: envelope.header.round,
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
            for_round: envelope.header.round,
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
            for_round: envelope.header.round,
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
        let round_claim = ProposalRoundClaimV1 {
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            for_round: envelope.header.round,
            block_hash,
        };
        let proposer_signature = SignatureEnvelope {
            algorithm_id: fixture.legitimate_proposer.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: fixture
                .legitimate_proposer
                .sign(&round_claim.signing_digest()?)
                .to_vec(),
        };
        let message = ConsensusProposalMessageV1 {
            for_round: envelope.header.round,
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
        let round_claim = ProposalRoundClaimV1 {
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            for_round: envelope.header.round,
            block_hash,
        };
        let proposer_signature = SignatureEnvelope {
            algorithm_id: fixture.legitimate_proposer.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: fixture
                .legitimate_proposer
                .sign(&round_claim.signing_digest()?)
                .to_vec(),
        };
        let message = ConsensusProposalMessageV1 {
            for_round: envelope.header.round,
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

    /// A real `FinalizedBlock` at `height`, padded with
    /// `evidence_blob_count` maximal-size evidence blobs purely to
    /// inflate its own encoded size -- `evidence` entries are opaque
    /// bytes at this layer (no concrete `ConsensusEvidence` shape
    /// exists yet, ADR-0015), so this is a cheap, honest way to build
    /// a genuinely large block without constructing thousands of real
    /// signed transactions.
    fn fat_finalized_block(height: u64, evidence_blob_count: usize) -> NodeResult<FinalizedBlock> {
        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions: Vec::new(),
            receipts: Vec::new(),
            evidence: vec![vec![0xab_u8; MAX_EVIDENCE_BLOB_LEN]; evidence_blob_count],
            extra_data: Vec::new(),
        };
        let header = BlockHeader {
            header_version: HEADER_VERSION_1,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            height: BlockHeight::new(height),
            round: Round::new(0),
            epoch: Epoch::new(0),
            protocol_epoch: ProtocolEpoch::GENESIS,
            parent_block_hash: [0_u8; 32],
            proposer: [0x01_u8; 32],
            timestamp: UnixTimeMillis::from_millis(height),
            transactions_root: body.transactions_root()?,
            state_root: [0x02_u8; 32],
            receipts_root: body.receipts_root()?,
            events_root: list_empty_root()?,
            consensus_root: DEVNET_VSC,
            evidence_root: body.evidence_root()?,
            protocol_parameters_hash: protocol_parameters_placeholder_hash()?,
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
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            epoch: Epoch::new(0),
            height: BlockHeight::new(height),
            round: Round::new(0),
            validator_set_commitment: DEVNET_VSC,
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

    #[test]
    fn fit_blocks_within_byte_budget_trims_64_fat_blocks_without_an_encoding_error()
    -> NodeResult<()> {
        // Each block carries 4 maximal evidence blobs (~256 KiB) --
        // 64 of them are ~16 MiB raw, comfortably over
        // `SYNC_RESPONSE_BYTE_BUDGET` (~8 MiB): exactly the shape
        // ADR-0044's own "Unbounded response size" finding named --
        // `MAX_SYNC_BLOCKS_PER_RESPONSE` alone would have let all 64
        // of these through and failed to encode at the P2P framing
        // layer.
        let blocks: Vec<FinalizedBlock> = (1..=64)
            .map(|height| fat_finalized_block(height, 4))
            .collect::<NodeResult<_>>()?;
        let trimmed = fit_blocks_within_byte_budget(blocks, SYNC_RESPONSE_BYTE_BUDGET)?;
        assert!(!trimmed.is_empty());
        assert!(trimmed.len() < 64);
        let encoded_len = SyncBlockRangeResponseV1 { blocks: trimmed }.encode()?.len();
        assert!(encoded_len <= SYNC_RESPONSE_BYTE_BUDGET);
        Ok(())
    }

    #[test]
    fn fit_blocks_within_byte_budget_always_includes_at_least_one_oversized_block() -> NodeResult<()>
    {
        // 128 maximal evidence blobs alone are already ~8 MiB -- with
        // this block's own other fields and the response's own
        // framing overhead, the single block exceeds
        // `SYNC_RESPONSE_BYTE_BUDGET` on its own. It must still be the
        // one block returned, never zero -- a lagging node must keep
        // making real progress even when the very next real block
        // happens to be this large.
        let huge_block = fat_finalized_block(1, 128)?;
        let huge_block_len = SyncBlockRangeResponseV1 {
            blocks: vec![huge_block.clone()],
        }
        .encode()?
        .len();
        assert!(huge_block_len > SYNC_RESPONSE_BYTE_BUDGET);

        let second_block = fat_finalized_block(2, 1)?;
        let trimmed = fit_blocks_within_byte_budget(
            vec![huge_block.clone(), second_block],
            SYNC_RESPONSE_BYTE_BUDGET,
        )?;
        assert_eq!(trimmed, vec![huge_block]);
        Ok(())
    }

    /// ADR-0046's own conformance requirement: "a blocked event loop
    /// gives `503 node_busy`." Nothing ever drains `event_rx` here --
    /// standing in for an event loop saturated with other real work
    /// that never reaches this particular `NodeEvent::Rpc` within the
    /// deadline. This is also the proof that consensus itself never
    /// shares this wait: `drive()` (the only thing that would ever
    /// read from this channel) never blocks on an `RpcHandler` in the
    /// other direction -- the wait below is entirely this handler's
    /// own `recv_timeout`, with no corresponding wait anywhere on the
    /// consensus side for it to stall.
    #[test]
    fn rpc_handler_returns_node_busy_when_the_event_loop_never_answers() {
        let (event_tx, _event_rx) = mpsc::channel::<NodeEvent>();
        let handler = RpcHandler { event_tx };
        let request = ParsedRequest {
            method: hn_rpc::HttpMethod::Get,
            target: "/v1/status",
            headers: Vec::new(),
            body: &[],
        };

        let started = std::time::Instant::now();
        let response = handler.handle(request);
        let elapsed = started.elapsed();

        assert_eq!(response.status, 503);
        assert_eq!(
            String::from_utf8(response.body).unwrap_or_default(),
            r#"{"error":"node_busy"}"#
        );
        assert!(elapsed >= RPC_REPLY_DEADLINE);
        assert!(elapsed < RPC_REPLY_DEADLINE + Duration::from_secs(1));
    }
}
