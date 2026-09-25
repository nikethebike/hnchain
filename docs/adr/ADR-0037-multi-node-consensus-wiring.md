# ADR-0037: Multi-Node Consensus Wiring

Status: Accepted

Date: 2026-09-25

Version: 0.1.0

Depends On:

- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0011: Leader Election
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0018: P2P Protocol Messages
- ADR-0019: Storage State Interfaces
- ADR-0033: Atomic Write-Set Commit
- ADR-0034: Consensus State Machine Skeleton
- ADR-0035: Wiring The Consensus Engine To `hn-state`
- ADR-0036: Basic P2P Networking

Supersedes: None

Referenced By:

- ADR-0038: Genesis Format And Node Daemon Bootstrap (replaces this
  ADR's own `--validator-index`/`--validator-count`/`--base-port`
  devnet arithmetic with real genesis-driven identity and an explicit
  peer/config flag set; also drops the `resolve_validator_index`/
  `conn_by_validator` connection-layer bookkeeping this ADR built,
  found not to be load-bearing for correctness)
- ADR-0039: Devnet Restart Recovery (adds connection-drop reconnection
  — this ADR's own dialer threads never retried a link once it dropped
  — and passive height-observation catch-up; both found missing only
  by actually killing and restarting a previously-connected node,
  which none of this ADR's own tests did)

## Context

`hn-consensus` (ADR-0034/ADR-0035) is a real, `hn-state`-wired Tendermint-style
state machine, proven end-to-end by a single validator producing a chain of
blocks entirely in one process (`single_validator_chain_integration.rs`).
`hn-network` (ADR-0036) is a real, tested set of P2P protocol types —
envelope, handshake, discovery, propagation payloads — with no real transport
and no live connection to `hn-consensus` at all.

Two pieces trusted as "someone else's future job" by both of those ADRs are
still missing:

- Vote aggregation: `ConsensusEngine::verify_vote` checks one vote at a time.
  Nothing collects individual votes into a new `QuorumCertificate`. Named
  explicitly in ADR-0035's own "Explicitly Not Resolved" and in
  `hn-consensus`'s own crate doc comment.
- Leader election: ADR-0011 decided the mechanism (deterministic weighted
  round-robin) but explicitly left the exact priority formula open. Nothing
  in this codebase picks a proposer at all yet — every existing test passes
  `block_hash`/`transactions` into `handle_proposal` directly, as if the
  caller already knew who should propose.

Proving real multi-validator consensus requires closing both gaps, plus a
real transport (`hn-network` has none) and a real process that drives the
state machine off wall-clock timers instead of a test calling
`propose_timeout()` by hand.

The user's own framing: several real OS processes on localhost, real voting
and quorum computed over the network (not in-process calls), and a first
real test of the timeout/view-change path specifically — not just the happy
path `single_validator_chain_integration.rs` already proved.

## Decision

### Decided: Transport — Blocking `std::net` + Threads

Asked explicitly (recommended option chosen): plain blocking TCP via
`std::net::{TcpListener, TcpStream}`, one OS thread per connection (a reader
thread and the accept loop; writes go through a per-connection
`mpsc::Sender<Vec<u8>>` drained by a dedicated writer thread), not `tokio`.

This continues ADR-0036's own reasoning for staying transport-free rather
than reopening it: no async runtime exists anywhere in this workspace yet,
and introducing `tokio` now would be a consequential, hard-to-reverse
dependency shaping every future crate that touches `hn-node` (`hn-rpc`
included), not just this pass's own scope.

### Decided: Wire Framing

TCP is a byte stream, not a message stream, so `hn-network`'s existing
`P2PMessageEnvelopeV1::encode()`/`decode()` bytes need explicit framing to
know where one message ends and the next begins: a 4-byte little-endian
`u32` length prefix, then exactly that many envelope bytes.
`MAX_FRAME_LEN = MAX_PAYLOAD_LEN + 4096` bounds the prefix itself (rejecting
an oversized length before allocating a buffer for it) — the "frame limit"
stage ADR-0018's own "Cheap Rejection" pipeline already names first, ahead of
envelope decode.

### Decided: Connection Topology — Directed Dial By `validator_id`

Every node's peer list names every other validator's address. Both sides
attempting to dial each other independently would race and could produce two
redundant connections to the same peer. Resolved deterministically, no
negotiation needed: a node dials a peer only if its own `validator_id` is
numerically less than the peer's; otherwise it waits for that peer's inbound
connection. Exactly one TCP connection per validator pair, decided the same
way without either side needing to know the other already decided it.

Every connection — inbound or outbound — completes ADR-0036's `Hello`
handshake (`HandshakeState`) before either side treats anything received on
it as a real consensus/gossip message.

### Decided: Vote Aggregation — `VotePool`

New `hn_consensus::VotePool`, embedded in `ConsensusEngine` alongside the
existing `proposed_blocks` cache. Keyed by `(height, round, vote_type)`, then
by `(target_type, target_hash)` within that. Each already-individually-
verified vote (`ConsensusEngine::verify_vote`'s own job, unchanged) is
recorded against its signer's bit position in `ordered_active_set` (the same
ascending-`validator_id` order `QuorumCertificate::verify_signatures`
already assumes); a duplicate vote from the same signer for the same target
is accepted idempotently (does not double-count voting power).

`ConsensusEngine::record_vote(vote, reader, ordered_active_set:
&[ValidatorRecordV1])` wraps `verify_vote` and `VotePool::insert` together
and returns `Option<QuorumCertificate>` — `Some` exactly once, the moment a
target's `signed_voting_power * 3 > total_voting_power * 2` (the same
threshold formula `QuorumCertificate`'s own type documentation already
states; not a new rule invented here), built directly in
`QuorumCertificate`'s own wire shape (`signer_commitment` bitmap +
`aggregate_proof` in ascending bit order) so it can be handed straight to
the existing `ConsensusEngine::handle_quorum_certificate` unchanged. Every
node builds its own `QuorumCertificate` independently, from whichever
`2f+1`-worth of votes it personally observed first — nodes are not required
to agree on which specific signers appear in each other's certificates, only
that each one is independently valid, the standard Tendermint-style gossip
model, not a designated-aggregator model (no such role is decided or needed
anywhere in this codebase, and a designated aggregator would just be a new
single point of liveness failure).

Votes are gossiped: every node broadcasts its own cast vote (`prevote`/
`precommit`) to every connected peer over `Channel::Consensus`
(`MessageType::ConsensusVote`) as soon as it casts it — nothing waits for a
proposer or any other intermediary to relay it.

Stale pool entries (old heights/rounds once a height finalizes) are not
pruned in this pass — an accepted simplification, not a correctness gap: an
unbounded-but-slow-growing map across one test run's few heights is
harmless, and real pruning policy belongs with whatever eventually decides
`hn-node`'s own memory/resource bounds.

### Decided: Leader Election Scaffold — Deterministic Round-Robin

`hn_consensus::round_proposer(ordered_active_set: &[Digest], height:
BlockHeight, round: Round) -> Digest` = `ordered_active_set[(height.get() +
round.get()) % ordered_active_set.len() as u64]`.

This is explicitly **not** ADR-0011's final decision: ADR-0011 itself
authorizes exactly this — "Static rotation may still be used as a simple
baseline for devnet or early testing if it is clearly marked non-
production" (its own "Rejected Options" section). The exact weighted-
priority arithmetic ADR-0011 deliberately left open stays open; this
function exists only so a real network of processes can agree on who
proposes each round without inventing unverified priority-accumulation math
from memory — the same oracle-verification caution this project has applied
to cryptographic values applies equally to an unverifiable "remembered"
formula for a mechanism ADR-0011 never pinned down. Both height and round
feed the rotation (not round alone) so the proposer does not repeat
identically at round 0 of every height.

### Decided: Round Timer / Driving Loop

`hn-node`'s per-height driving loop is a single thread consuming one
`mpsc::Receiver<NodeEvent>`:

```text
enum NodeEvent {
    PeerMessage { peer_id, envelope },
    ProposeTimeout { height, round },
    PrevoteTimeout { height, round },
    PrecommitTimeout { height, round },
}
```

Every connection's reader thread and a per-stage timer thread all hold
clones of the same `Sender`. Entering `Propose`/`Prevote`/`Precommit`
spawns a fresh timer thread that sleeps for that stage's timeout, then sends
its tagged timeout event; the main loop ignores any timeout event whose
`(height, round)` no longer matches `engine.state()`'s current one (the
round already advanced or the height already finalized by the time the
timer fired) — the same "stale event, not an error" handling ADR-0034's own
event model already expects callers to do, made concrete for real wall-clock
timers instead of a test calling the transition method directly.

**Correction (found in ADR-0038's own testing, not caught here):**
`(height, round)` alone is not a sufficient staleness check — a node
that is also the current round's proposer self-proposes and casts a
`Prevote` synchronously, in the same call that entered `Propose`,
before that stage's own `ProposeTimeout` timer ever fires. The stale
timer is still for the *same* `(height, round)`, just a step the engine
has already left, so it must also be checked against the engine's
*current step*, not only its height/round — see ADR-0038's own
implementation notes in `hn-node/src/node.rs` for the fix. This pass's
own multi-node happy-path tests never exercised it: real quorums
arrived well within one timeout window, so by the time a stale timer
fired the round had already advanced past it for an unrelated (but
also correct) reason.

### Decided: Timeout Duration — Implementation Default, Not ADR-0009's Final Value

`base_timeout_ms` (CLI-configurable, default `500`), backoff `base_timeout_ms
* (round.get() + 1)` per stage. ADR-0009's own "Timeout And View Change"
section already states the exact base value and backoff formula are a
distinct, later, tunable-parameter decision, not a structural one this ADR
is positioned to make. This pass needs *some* concrete, working value to
drive a real timer thread; picking one, config-overridable, explicitly not
claimed as ADR-0009's own final constant, matches exactly how
`TARGET_BLOCK_TIME` and `round_proposer` above are both handled — a real,
working, clearly-labeled-non-final default.

### Decided: Devnet Validator Identity — Deterministic, Index-Derived

For this pass's node configuration only (not a validator-onboarding
mechanism, which remains undecided anywhere in this codebase — ADR-0010
never defines `validator_id` derivation): `validator_id = [validator_index;
32]`, consensus key `Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus,
[validator_index; 32])`, network/handshake key
`Ed25519KeyPair::from_seed(KeyRole::ValidatorNetwork, [validator_index;
32])` — reusing `KeyRole::ValidatorNetwork` (already scoped to "validator
peer-identity / network-layer operations") rather than ADR-0036's more
general `KeyRole::NodeIdentity`, since every node in this pass genuinely is
a validator. This exact deterministic-seed convention already appears
throughout this session's own test code (`keypair(0x01)` and friends); using
it for real process configuration is a continuation of an established
pattern, not a new one.

### Decided: `hn-node` Process

A real binary (`hn-node/src/main.rs`), CLI-configured (`--validator-index`,
`--validator-count`, `--base-port`, `--chain-id`, `--network-id`,
`--data-dir`, `--base-timeout-ms`; hand-parsed — no CLI-parsing dependency
added, matching the minimal-dependency posture this ADR's transport decision
already took). Listens on `127.0.0.1:{base_port + validator_index}`; every
other validator's address is derived the same way from `base_port` and its
own index, so one shared `--base-port` plus each process's own
`--validator-index` fully determines the whole cluster's topology with no
separate peer-list file.

On startup: opens a `hn_storage::RedbStateStore` at `--data-dir`, writes
every validator's `ValidatorRecordV1` (`Active`, equal voting power) if not
already present (idempotent devnet genesis — not ADR-0019's own decided
genesis mechanism, which remains a distinct, later piece of work), dials/
accepts connections per the topology decision above, completes handshakes,
then begins height `BlockHeight::GENESIS` once connected to every configured
peer (or a bounded startup grace period elapses — a node that never reaches
every peer still begins; it simply cannot reach quorum alone, the same
liveness-not-safety property real Tendermint networks have).

Blocks in this pass carry zero transactions — proving liveness and view
change is this ADR's own stated goal, not state transitions, which
`single_validator_chain_integration.rs` already covers end-to-end. A
finalized block prints one line to stdout
(`FINALIZED height=<h> round=<r> block=<hex> signed_power=<n>/<total>`) —
the multi-process integration test's own observation mechanism, reading
each child process's piped stdout rather than reaching into another
process's memory or database file while it may still be writing to it.

### Decided: Multi-Process Integration Test

`hn-node/tests/multi_node_view_change.rs`. Four validators (`n=4`, `f=1`,
quorum `= 3`), spawned as three real `std::process::Command` child processes
of the compiled `hn-node` binary (`env!("CARGO_BIN_EXE_hn-node")`) — the
round-0/round-1-by-height-0-rotation proposer (validator index chosen so
`round_proposer` selects it at round 0) is deliberately never started,
simulating a dead/unreachable leader without needing an explicit kill
signal mid-test. The three running validators hold 3 of 4 equal shares of
voting power (75%), comfortably above the `2f+1` threshold, so the *absence*
of the fourth is exactly the kind of partial-participation case ADR-0010's
own byzantine-fault-tolerance model is meant to survive — this is not a
3-of-3 test wearing a 4-validator label.

Asserted, by reading piped stdout from each child process with a bounded
wait: no `FINALIZED height=0 round=0 ...` line ever appears (the dead
proposer's round never produces one); every running process eventually
prints `FINALIZED height=0 round=1 ...` for the same `block`/`signed_power`
values (independently-built quorum certificates over the same target,
proving real cross-process agreement, not a shared in-memory value); a
second height (`height=1`) finalizes afterward too, proving the chain
continues past the recovered round rather than stalling right after one
view change. Child processes are killed at test teardown regardless of
outcome.

## Explicitly Not Resolved

- ADR-0011's real weighted-priority proposer formula (this pass's
  `round_proposer` is explicitly its own sanctioned devnet placeholder, not
  a step toward deriving the real one).
- ADR-0009's final base timeout/backoff constants (this pass's default is a
  working, overridable implementation value, not a protocol constant).
- Real block header/receipts storage (`ConsensusEngine::proposed_blocks`
  remains the same transient cache ADR-0035 already described; this pass
  does not change that).
- Byzantine/equivocation handling: a validator signing two different votes
  for the same `(height, round, vote_type)` is not detected or penalized —
  `VotePool` simply records whichever arrives, `signer_commitment` bit
  already set means a later differing vote from the same signer for a
  *different* target in the same round is currently just ignored by
  `VotePool::insert`'s idempotency rule, not flagged as equivocation
  evidence (ADR-0015's own future job).
- Real validator onboarding/bonding (`--validator-index`-derived identity is
  devnet-only configuration, not a mechanism any real network would use).
- Peer reconnection after a dropped TCP connection mid-run; this pass's
  transport does not retry a connection once established and then lost.
- TLS/connection encryption (ADR-0018's own still-open "encryption
  profiles").
- Any RPC, CLI, or operator-facing surface for a running node beyond stdout
  log lines.

## Rejected Options

### `tokio` Async Runtime

Rejected for the same reason ADR-0036 already gave when explicitly deferring
it: no async dependency exists anywhere in this workspace yet, and adopting
one now would shape every future `hn-node`/`hn-rpc` addition, not just this
pass. Asked explicitly; blocking threads was the chosen, recommended option.

### A Designated Vote-Aggregator Role

Rejected: having only the current round's proposer collect votes and
broadcast the resulting `QuorumCertificate` would reduce message count, but
makes that one node's liveness a new single point of failure distinct from
(and additional to) the proposer-liveness case this ADR's own test already
exercises. Every node independently aggregating its own quorum certificate
from gossiped votes has no such bottleneck and matches Tendermint's own
standard model.

### Killing A Started Process Mid-Test Instead Of Never Starting It

Rejected as this pass's mechanism for simulating an unavailable proposer:
functionally similar (both leave the network without that validator's
messages), but never starting the process is simpler, fully deterministic
(no race on exactly which point in the process's lifecycle a kill signal
lands), and just as real a "the proposer is unavailable" condition from
every other node's point of view.

### Config File Instead Of CLI Flags Plus Index Arithmetic

Rejected for this pass: a real config-file format is its own future
decision (this ADR does not attempt to define `hn-node` configuration in
general). Deriving the whole cluster's topology from one shared
`--base-port` and each process's own `--validator-index` needs no shared
file at all and keeps the integration test's process-spawning code trivial.

## Security Considerations

Unauthenticated connections before handshake:

- Risk: a node processes bytes from an unverified peer.
- Mitigation: every connection completes ADR-0036's `Hello` handshake before
  anything received on it is treated as a real message — unchanged from
  ADR-0036, now actually exercised over a real socket for the first time.

Oversized frames:

- Risk: a malicious or buggy peer sends an unbounded length prefix, forcing
  a large allocation before any content is even read.
- Mitigation: `MAX_FRAME_LEN` is checked against the length prefix itself,
  before reading the frame body.

Vote flooding:

- Risk: a peer sends an excessive volume of (individually valid or invalid)
  votes to exhaust CPU on signature verification or memory in `VotePool`.
- Mitigation: none added in this pass — named explicitly as an accepted gap;
  real rate limiting is ADR-0018's own still-open "rate limit policy."

Dead proposer liveness:

- Risk: an unavailable or byzantine proposer stalls the chain.
- Mitigation: exactly what this ADR's own test proves survives it — propose
  timeout, round advance, rotated proposer, and a quorum among the
  remaining honest, connected validators.

## Compatibility

This ADR does not change any wire format decided by ADR-0012 or ADR-0018/
ADR-0036 — `VotePool` only builds `QuorumCertificate` values in the exact
shape already decided, and the TCP framing prefix sits entirely outside the
envelope's own canonical bytes (never hashed, never signed, purely a
transport-level delimiter). A future real transport (or `tokio` adoption)
could replace this pass's `std::net` implementation without touching
anything `hn-consensus` or the wire formats define.

## Open Decisions

- ADR-0011's real proposer priority formula (unchanged, still open)
- ADR-0009's real timeout base/backoff constants (unchanged, still open)
- connection retry/reconnection policy
- vote-pool pruning/eviction policy
- rate limiting and peer scoring (ADR-0018, unchanged, still open)
- real `hn-node` configuration format (file-based or otherwise)
- byzantine/equivocation detection and evidence (ADR-0015)

## Related Specifications

- `docs/rfc/consensus/consensus-architecture.md`
- `docs/rfc/consensus/leader-selection.md`
- `docs/rfc/consensus/vote-messages-and-quorum-certificates.md`
- `docs/rfc/networking/p2p-protocol-messages.md`
