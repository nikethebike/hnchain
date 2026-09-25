# ADR-0039: Devnet Restart Recovery

Status: Accepted

Date: 2026-09-25

Version: 0.1.0

Depends On:

- ADR-0034: Consensus State Machine Skeleton
- ADR-0037: Multi-Node Consensus Wiring
- ADR-0038: Genesis Format And Node Daemon Bootstrap

Supersedes: None

Referenced By: None

## Context

The user's own acceptance criterion for "the first real multi-node
devnet": kill a node, bring it back up, and it recovers the network's
actual current state on its own — not just reconnects, actually catches
up and rejoins consensus.

`hn-node` (ADR-0037/ADR-0038) already boots a real, genesis-driven,
multi-process validator network. Manually testing the exact scenario
this ADR is named for — kill one of four running validators mid-run,
restart it against the same `--data-dir`, confirm it rejoins — surfaced
two real gaps neither prior ADR had actually exercised, since neither
one's own tests ever killed and restarted a node that had previously
been connected:

1. **No reconnection.** `ConsensusEngine` always starts fresh at
   `BlockHeight::GENESIS` on every process start (there is no persisted
   "current height" anywhere, matching this pass's own scope — see
   "Decided: No Height Persistence," below). A restarted node's own
   in-memory height is stale the instant its peers have progressed past
   genesis, so it needs to actually learn the network's current height
   from somewhere. Testing this directly surfaced a *second*, more
   basic problem first: `hn-node`'s dialer threads (ADR-0037) connect
   once and exit; nothing re-dials a peer whose connection later drops.
   A restarted node's *former* peers — already connected to it before
   it died — never attempt to reconnect once its process comes back,
   because their own dialer threads for it exited the moment the
   original connection succeeded, long before it died. The restarted
   node sat completely isolated, forever, producing no output at all.
2. **Stale-proposal misattribution.** `ConsensusProposalMessageV1`
   (ADR-0037) carries no `height`/`round` of its own — a receiver could
   only check whether its own engine happened to be at the `Propose`
   step, not whether a given proposal actually targeted its own current
   round. A restarted (and therefore height-stale) node broadcasting a
   proposal for its own bogus low height into an already-advanced
   network could be misapplied by a peer that happened to also be at
   `Propose` for a *different* round.

Both are closed here, plus the actual catch-up mechanism a restarted
node uses once reconnected.

## Decision

### Decided: `ConsensusProposalMessageV1` Self-Describes `height`/`round`

Two new fields, `height: BlockHeight` and `round: Round`, added ahead of
`block_hash`. `hn_consensus::ConsensusEvent::Proposal` itself correctly
carries neither (ADR-0034's own trusted-input boundary: it always
targets whichever round the local `ConsensusState` is already
attempting) — the gap was specifically at the network boundary, where
"whichever round `ConsensusState` is attempting" and "the round this
*specific* proposal was actually produced for" are two different things
once messages can be stale, out of order, or (now) coming from a
recently-desynchronized node. `ConsensusVote`/`QuorumCertificate` never
had this gap; they already self-describe their own `height`/`round`.
`hn-node`'s own receiving logic now checks `message.height`/
`message.round` against its engine's *exact* current `(height, round,
step == Propose)` before ever calling `handle_proposal` — strictly
stronger than the step-only check this replaces.

### Decided: Connection-Drop Reconnection

`hn_network::spawn_peer_link` gains an `on_close: Sender<L>` parameter:
its reader thread sends `label` there when its read loop ends (peer
disconnected, or sent malformed bytes), before the thread exits. The
writer thread gets no separate signal — a dead connection's read side
failing is already the reliable single source of truth this function
reports from; a second signal from the write side would only ever
duplicate it, never add new information.

`hn-node` uses this to know when a connection it *initiated* (a
directed dial, ADR-0037's own listen-address ordering) has died, by
recording `conn_id -> dialed address` only for connections it dialed
itself (`NodeEvent::NewConnection` gains a `dial_target: Option
<SocketAddr>` field). On `NodeEvent::ConnectionClosed`, if this
connection was one this node dialed, it redials the same address —
exactly the same `spawn_dialer` function used at startup, called again
from scratch, not a persistent supervisor thread that outlives one
connection attempt. A dropped *inbound* connection triggers no redial
here: reconnecting it is the dialing peer's own responsibility, the
same directed-dial symmetry ADR-0037 already established (exactly one
side of every pair dials, the other only accepts) extended to also
cover reconnection, not just the first connection.

This was not a hypothetical gap closed defensively — restarting a node
that had previously been connected produced total, silent isolation
before this fix: its former peers' dialer threads had already exited
after their first successful connection, long before the restart, and
nothing else in the codebase ever attempted to reach it again.

### Decided: Passive Height-Observation Catch-Up

No dedicated sync request/response message, and no extension to the
closed `Channel`/`MessageType` registries (ADR-0036) at all.
`ConsensusVote`/`QuorumCertificate`/(now) `ConsensusProposalMessageV1`
already self-describe their own `height`; a rejoining node starts
observing them the moment any reconnected link's handshake completes
and the network's already-constant vote gossip reaches it (real
quorums in this devnet's happy path form and re-gossip within a single
round timeout, so this happens quickly in practice). Whenever a
received vote, certificate, or proposal names a height strictly ahead
of this node's own current one, `hn-node` replaces its
`ConsensusEngine` outright with `ConsensusEngine::new_height` at the
observed height (round `0`, no lock — the same fresh-height shape any
new height already starts from) and immediately begins a round there,
rather than replaying every intermediate height's content.

This shortcut is valid specifically *because* this devnet's own blocks
are always empty (ADR-0037's own deliberate scope: proving liveness/
view-change, not state transitions) — there is no actual application
state to reconstruct height-by-height, only a height/round position to
catch up. A future pass with real transactions in blocks would need
real block/state sync (ADR-0016's own still-entirely-open territory:
checkpoints, snapshots, verified historical replay) — this is
explicitly not that, and is not positioned as a step toward it.

If the jump lands at round `0` while peers have already moved past it
at that height (this node caught up mid-round, not at its very start),
this node's own round-`0` attempt simply times out through the normal
propose/prevote/precommit cycle, like any other failed round, and
naturally advances the rest of the way one round at a time. Slower than
jumping straight to the observed round, but needs no new
`ConsensusEngine` constructor and reuses every existing mechanism
unchanged.

Every jump is strictly forward (`observed_height` must exceed the
current one) — never triggered by an already-caught-up or genuinely
stale observation, so local progress stays monotonic even under
out-of-order message delivery from several peers at once.

### Decided: No Height Persistence

`hn-node` does not persist "current height" to `RedbStateStore`
anywhere. The passive catch-up mechanism above makes it unnecessary for
this pass's own goal: as long as at least one connected peer is ahead,
a restarted node learns the real height from the network itself, every
time, without needing its own last-known position on disk at all. A
future pass could still add local height persistence as a pure
optimization (skip waiting for a peer message if a node already knows
where it left off) — deliberately not attempted here, since the
passive mechanism alone already satisfies the actual acceptance
criterion this ADR was written to meet, confirmed by the manual restart
test below.

### Decided: Flushed Log Lines

`hn-node`'s two operationally significant log lines (`FINALIZED`,
`SYNCED`) now go through a small `log_line` helper that explicitly
flushes stdout after every write. Found while manually testing this
exact restart scenario: a process killed shortly after producing output
could lose that output entirely if it was still sitting in an unflushed
buffer — directly undermining this ADR's own restart-recovery testing,
and more generally the only operational visibility this daemon has,
given ADR-0038's own deliberate choice not to install a graceful
shutdown handler. A `kill`, at any moment, remains an expected, ordinary
way to stop this process; its last log line reaching disk before that
happens should not depend on buffering timing.

## Explicitly Not Resolved

- real block/state sync for a future network with non-empty blocks
  (ADR-0016's own territory, unchanged and untouched)
- local height persistence as a startup-time optimization
- byzantine/equivocation handling (unchanged from ADR-0037)
- reconnection backoff/backpressure tuning (the existing flat 100ms
  retry, unchanged from ADR-0037, is reused as-is for redialing)
- any bound on how many times a connection may flap and be redialed

## Rejected Options

### A Dedicated Height-Query Request/Response Message

Rejected: `ConsensusVote`/`QuorumCertificate`/`ConsensusProposalMessageV1`
already carry everything a passive observer needs, and the network's
own steady-state gossip already delivers them continuously — a
dedicated query message would duplicate information already in transit
for an unneeded round trip, and would need extending the closed
`MessageType` registry (ADR-0036) for a capability the passive approach
gets for free.

### A Persistent Per-Peer Reconnection-Supervisor Thread

Rejected in favor of calling `spawn_dialer` again from the single
driving loop on `NodeEvent::ConnectionClosed`: a supervisor thread that
outlives one connection attempt would need its own state machine to
avoid redialing a link that is still alive, duplicating exactly the
bookkeeping (`established`, `dialed_peers`) the driving loop already
owns. Re-invoking the same startup-time function from the one place
that already tracks connection state keeps reconnection and initial
connection using identical logic, not two parallel implementations.

### Height/Round-Aware Direct Round Jump

Rejected as unnecessary complexity for this pass: jumping straight to
an observed `(height, round)` pair (rather than always round `0`) would
need a new `ConsensusEngine` constructor and would only save a few
timeout cycles' worth of catch-up time in a devnet already running
short round timeouts — the existing propose/prevote/precommit timeout
cycle already gets a resyncing node the rest of the way with no new API
surface.

## Security Considerations

Unbounded reconnection attempts:

- Risk: a dead or unreachable peer causes indefinite 100ms-interval
  redial attempts forever.
- Mitigation: none added this pass — the same flat-retry posture
  ADR-0037's own original dialer already had; named explicitly as an
  accepted gap ("Explicitly Not Resolved," above), not silently
  carried over unremarked.

Height-jump spoofing:

- Risk: a malicious peer sends a vote or QC naming a wildly inflated
  height to force a victim to jump forward and abandon real, honest
  progress.
- Mitigation: none beyond what already exists — `record_vote`/
  `handle_quorum_certificate`'s own signature verification against
  genesis-loaded consensus keys still applies before a vote/QC is ever
  trusted for anything else, but the height *value* itself, once a
  message's signature is otherwise valid, is trusted as-is by the sync
  check. Acceptable for a closed, known-validator devnet; a real
  network would need this analyzed as part of whatever real sync
  mechanism eventually replaces this pass's shortcut.

## Compatibility

`ConsensusProposalMessageV1`'s new fields are a breaking wire-format
change to an ADR-0037-decided type with no external deployment to
preserve compatibility with yet (this whole project is pre-mainnet,
and this message type has had exactly one implementation, this
codebase's own). `spawn_peer_link`'s new `on_close` parameter is a
breaking Rust API change to an `hn-network`-internal function with a
single real caller (`hn-node`), updated in the same pass.

## Open Decisions

- real block/state sync (ADR-0016, unchanged)
- reconnection backoff policy
- local height persistence

## Related Specifications

- `docs/adr/ADR-0016-synchronization-checkpoints.md`
