# ADR-0044: Full-Verification Block And State Sync

Status: Proposed

Date: 2026-10-12

Version: 0.1.0

Depends On:

- ADR-0008: Block Format
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0013: Finality Rules
- ADR-0016: Synchronization Checkpoints
- ADR-0018: P2P Protocol Messages
- ADR-0030: Transaction And Block Application
- ADR-0036: Basic P2P Networking
- ADR-0039: Devnet Restart Recovery
- ADR-0042: BlockEnvelope And Receiver-Side Header Verification
- ADR-0043: Genesis As A Real Block 0

Supersedes: None (narrows ADR-0039's own "Decided: Passive
Height-Observation Catch-Up" to the always-empty-block devnet it was
built for — see "Compatibility," below — without touching ADR-0039's
separate connection-drop-reconnection decision at all)

Referenced By: None

## Context

ADR-0016 ("Synchronization Checkpoints") already requires, as its own
"Full Verification Mode" normative rule: "HNChain must support full
verification from genesis. Checkpoint-assisted synchronization is an
optimization, not the only security mode." Nothing has ever
implemented that mode, concretely or abstractly — ADR-0016 itself
stayed at the conceptual level (`SyncCheckpoint` fields, weak
subjectivity policy, snapshot manifests), and every later pass that
touched catch-up behavior explicitly deferred to it by name rather
than resolving it:

- ADR-0039's own "Decided: Passive Height-Observation Catch-Up" states
  plainly: "This shortcut is valid specifically *because* this
  devnet's own blocks are always empty... A future pass with real
  transactions in blocks would need real block/state sync (ADR-0016's
  own still-entirely-open territory)... this is explicitly not that,
  and is not positioned as a step toward it." A rejoining node today
  jumps its own height counter forward the moment it observes a higher
  height in ordinary gossip — it never fetches, verifies, or replays
  the content of any block between where it was and where it jumped
  to, because there has never been any real content to miss.
- ADR-0042's own crate doc comment names "real block/state sync for a
  future network with non-empty blocks (ADR-0016, unchanged, including
  this node's own inability to verify `parent_block_hash`/timestamp
  monotonicity immediately after a passive catch-up jump)" as
  explicitly out of scope.
- `hn-network`'s own `Channel` registry (ADR-0036) already reserves
  `Sync = 0x07` with the comment "Reserved: blocked on ADR-0016's own
  sync packet design" — a slot waiting for exactly this decision.
  `MessageType` already reserves `CheckpointRequest`/
  `CheckpointResponse`/`SnapshotManifest`/`SnapshotChunkRequest`/
  `SnapshotChunkResponse` for ADR-0016's own checkpoint/snapshot
  packet design specifically — but reserves nothing yet for the plainer
  "fetch a range of real, already-finalized blocks and replay them"
  protocol Full Verification Mode itself needs. That gap, not the
  checkpoint/snapshot machinery, is this ADR's own scope.

This ADR resolves Full Verification Mode concretely — it does not
resolve ADR-0016's other modes (checkpoint-assisted, snapshot-assisted,
weak subjectivity, light clients), which remain exactly as open as
before and are explicitly not attempted here (see "Explicitly Not
Resolved," below). This is a deliberate, user-directed scope choice:
full verification is sufficient on its own for a testnet's correctness
and safety; the other modes are UX/performance optimizations layered
on top later, not blockers.

Real transactions still never flow through consensus-produced blocks
anywhere in this codebase (ADR-0037's own deliberate, still-unchanged
scope) — this ADR specifies a mechanism general enough to replay real,
non-empty block content once that exists, but cannot itself be
exercised against anything but empty blocks today. It is written and
reviewed now, as a spec, deliberately ahead of both that prerequisite
and its own implementation — see "Open Decisions."

## Decision

### Decided: Scope Is Full Verification Mode Only

This ADR specifies how a node fetches, verifies, and replays every
real block between its own last known-good height and a peer's
observed current height, starting from genesis (ADR-0043) or from
wherever the node's own local history already reaches. It does not
specify checkpoints, snapshots, weak subjectivity, or light-client
proofs — ADR-0016's own conceptual sketches for those remain the
forward-looking record of that still-entirely-open future work.

### Decided: Durable Finalized-Block Storage

Every validating node persists every block it finalizes — no pruning,
no archive/full-node distinction yet (ADR-0038's own already-deferred
"non-validating full node mode" item covers that future distinction;
this ADR does not reopen it). Without this, there is nothing for a
synced-up node to ever serve a lagging peer.

A new type, `hn_state::FinalizedBlock`:

```rust
pub struct FinalizedBlock {
    pub envelope: BlockEnvelope,
    /// The real `precommit` quorum certificate that finalized this
    /// block (ADR-0013's own "one precommit QC is sufficient" rule) —
    /// already produced today, by `ConsensusAction::Finalized.
    /// commit_qc`, and already discarded today (`act_on`'s `Finalized`
    /// branch destructures `{ block_hash, .. }`). This ADR does not
    /// invent a new "finality proof" concept; it persists a value
    /// that already flows through the live system.
    pub commit_qc: QuorumCertificate,
}
```

Stored in `hn_storage`, keyed by `height: u64`, alongside (not
replacing) the existing `RedbStateStore` account/validator state table
— a new table in the same backend, not a new crate or a parallel
store with its own consistency story. No secondary `block_hash ->
height` index: this ADR's own sync protocol is purely height-ordered
(see "Decided: Wire Protocol," below), and nothing else in this
codebase needs hash-keyed historical block lookup yet.

`hn-node`'s own `Ctx.pending_headers: HashMap<Digest, BlockHeader>`
(ADR-0042) generalizes to `Ctx.pending_envelopes: HashMap<Digest,
BlockEnvelope>`, caching the whole verified envelope a proposal
carried, not just its header — needed so the full envelope (not only
its hash) is available to persist as a `FinalizedBlock` at the moment
`act_on`'s `Finalized` branch succeeds, using the very `commit_qc`
that action already carries.

### Decided: Wire Protocol

Two new `MessageType` values under the already-reserved `Channel::Sync
= 0x07`:

```rust
/// "Send me finalized blocks starting after `from_height`, up to
/// `max_count` of them."
SyncBlockRangeRequest = 0x0019,
/// A bounded, height-ordered run of `FinalizedBlock`s, starting
/// immediately after the request's own `from_height` — a prefix of
/// what was asked for if the responder does not have (or will not
/// send) the full range.
SyncBlockRangeResponse = 0x001A,
```

```rust
pub struct SyncBlockRangeRequestV1 {
    pub from_height: BlockHeight,
    pub max_count: u32,
}

pub struct SyncBlockRangeResponseV1 {
    pub blocks: Vec<FinalizedBlock>,
}
```

`max_count` is bounded by a new `MAX_SYNC_BLOCKS_PER_RESPONSE`
implementation resource bound (same class of decision as
`MAX_TRANSACTIONS_PER_MESSAGE`) — a large gap is fetched as several
requests in sequence, not one unbounded message. The responder answers
from its own durable `FinalizedBlock` table directly; a responder
missing part of the requested range (a non-validating full node with
partial retention, once that mode exists) sends whatever contiguous
prefix it actually has, never a gap-containing or reordered one.

This is a distinct channel and distinct message family from
`Channel::Blocks`' existing `BlockAnnounceV1`/`BlockRequestV1`/
`BlockResponseV1` (ADR-0036) on purpose: those name one block by hash,
for a receiver that already knows which hash it wants (today, dormant
— wired nowhere in `hn-node`); this protocol names a height range, for
a receiver that knows only that it is behind, not which hashes it is
missing. Resolving `BlockRequestV1`/`BlockResponseV1`'s own now-stale
doc comments ("no real `BlockHeader` exists anywhere in this codebase
yet") and whether to wire or retire them is a separate, smaller
cleanup this ADR does not attempt.

### Decided: Verification Procedure

A fetched `FinalizedBlock` is accepted only if, in order:

1. `envelope.block_hash()` is recomputed from the received header —
   never trusted as a separately-carried claim (ADR-0042's own
   principle, applied here to historical rather than live data).
2. `envelope.header.height` is exactly one more than the last block
   this node has itself already accepted (sequential, gapless,
   starting from genesis or from wherever local history already
   reaches) — `envelope.header.parent_block_hash` must equal that
   prior block's own real `block_hash`. Full replay from genesis means
   this chain of equalities is unbroken all the way back to ADR-0043's
   own genesis header, not merely checked one block at a time in
   isolation.
3. `envelope.verify_body_matches_header()` (ADR-0042) — the header's
   claimed roots/hashes actually match the body it was sent with.
4. **`commit_qc` is a real `precommit` quorum certificate for exactly
   this `block_hash`, at exactly this `height`/`round`, verified via
   `QuorumCertificate::verify_signatures` against the active set at
   that height.** This is the one requirement live proposal
   verification (ADR-0042) does not have and historical sync
   verification must: a live receiver trusts a legitimately-signed,
   eligible proposer *because* it will go on to watch real-time votes
   accumulate into its own quorum. A node fetching history was not
   there for that; a proposer's signature alone proves only that the
   proposer proposed something, never that `2f+1` of the active set
   actually finalized it. Without a real, verified `commit_qc`, a
   single dishonest peer could serve an entire fabricated history that
   no honest validator ever actually finalized.
5. The block's transactions are replayed via the existing
   `apply_and_commit_block` (ADR-0030, unchanged) against this node's
   own current state. The resulting `transactions_root`/
   `receipts_root` must match the header's own claimed values — not
   merely re-checked for formatting consistency (step 3 already did
   that against the *claimed* body) but now independently *re-derived*
   by actually executing every transaction. A full state-root
   recomputation after replay is the rigorous version of this same
   check; given this devnet's own blocks are still always empty,
   `state_root` provably cannot change yet (ADR-0040's own reasoning,
   unchanged), so there is nothing to recompute today — a real,
   non-empty-block network would need this to actually run, named
   here rather than silently assumed away.

Any failure at any step aborts the sync attempt against that specific
peer — logged, not fatal to the process (the same "externally-
triggerable input must not crash this process" principle ADR-0042
already established) — and the node tries a different peer, the same
general resilience posture `hn-node`'s own dialer/reconnection logic
(ADR-0039) already has for connections.

### Decided: Replacing The Passive Jump For Real Catch-Up

`maybe_sync_forward` (ADR-0039) no longer jumps `ConsensusEngine`
straight to an observed height, discarding everything in between. On
observing a height strictly ahead of its own, a node instead issues
`SyncBlockRangeRequestV1`s (repeating as needed for a large gap) and
replays each returned, verified `FinalizedBlock` in order via
`act_on`'s own existing `Finalized`-handling path (reusing it, not
duplicating it) until caught up to the observed height, *then* begins
a real round there. This closes ADR-0042's own named gap directly:
`parent_block_hash`/timestamp-monotonicity verification is no longer
skipped after catching up (`Ctx.chain_position_verified` does not need
its own escape hatch for this case at all once sync genuinely replays
every intervening block instead of skipping them) — the real chain of
custody back to genesis is exactly as unbroken for a node that just
caught up as for one that was running the whole time.

ADR-0039's own connection-drop reconnection decision is untouched —
only its passive-jump catch-up mechanism is replaced, and only once
this ADR's own mechanism is actually implemented (see "Open
Decisions").

## Rejected Options

### Checkpoint- Or Snapshot-Assisted Sync Now

Rejected (the user's own call): ADR-0016's own text already frames
these as optimizations over full verification, not substitutes for
it — a testnet needs the substitute's own foundation (full
verification) to exist at all before an optimization on top of it is
worth specifying. Revisit once full verification mode is implemented
and real transaction volume makes replaying from genesis genuinely
slow.

### Retrofitting `BlockRequestV1`/`BlockResponseV1` Into A Range Protocol

Rejected: those types name one block by hash, a different shape of
problem (a receiver that already knows what it wants) from "I am
behind and do not know which hashes I am missing." Reusing them would
mean either a hash-discovery side-channel this ADR does not otherwise
need, or awkwardly overloading `block_hash` fields with range
semantics they were never designed to carry.

### Hash-Indexed Durable Block Storage

Rejected for now: nothing in this protocol or this codebase needs
hash-keyed historical lookup — `SyncBlockRangeRequestV1` is purely
height-ordered, and height is already how every other part of this
system (consensus itself, `Ctx.last_block_hash` chaining) addresses
blocks. A hash index can be added later without a storage-format
break if something new actually needs one.

### Recomputing `state_root` As A Mandatory, Immediate Check Today

Rejected as a *requirement right now*: this devnet's blocks are
provably always empty, so there is nothing for a full recomputation to
actually catch today — mandating the expensive operation anyway would
be ceremony, not a real safety improvement, for content that cannot
exist yet. Named explicitly as needed once real transactions exist
(see "Decided: Verification Procedure," step 5), not silently dropped.

## Compatibility

Narrows ADR-0039's own "Decided: Passive Height-Observation Catch-Up"
to networks whose blocks are still always empty (today's devnet) —
ADR-0039's text already said this shortcut "is explicitly not [a step
toward real sync], and is not positioned as" one, so this is not a
reversal of that decision, only the arrival of the thing it always
said would eventually be needed instead. `Channel::Sync`/the two new
`MessageType` values are new wire surface, additive to the closed
registries (ADR-0036) — no existing message type's own byte value
changes.

## Security Considerations

A dishonest peer serving fabricated history:

- Risk: a single malicious responder sends a plausible-looking but
  never-actually-finalized chain of blocks.
- Mitigation: step 4's `commit_qc` signature verification — a block
  without a real `2f+1` precommit quorum from the active set at that
  height is rejected outright, regardless of how well-formed its
  header/body otherwise look.

A dishonest peer serving a valid prefix then stopping, or an
unavailable/partial responder:

- Risk: a syncing node cannot make progress against one peer.
- Mitigation: per-peer failure is not fatal; a node tries another peer
  for the remaining range (mirrors existing dialer/reconnection
  resilience, ADR-0039).

Unbounded response size:

- Risk: a malicious or buggy responder sends an enormous single
  response.
- Mitigation: `MAX_SYNC_BLOCKS_PER_RESPONSE` bounds every response; a
  larger gap is fetched across multiple bounded requests.

Validator-set evolution during the synced range:

- Risk: step 4 verifies `commit_qc` against "the active set at that
  height," but this codebase has no real validator-set-rotation
  mechanism yet (ADR-0010's own still-open item) — a future network
  where the active set actually changes over time would need this
  step to resolve the *correct historical* active set, not just the
  current one.
- Mitigation: none needed yet — this devnet's validator set is fixed
  from genesis, so "the active set at that height" and "the current
  active set" are provably the same set today. Named as a real,
  future-relevant boundary, not silently assumed to stay true forever.

## Open Decisions

- the actual implementation (storage table, wire types' concrete HNCS
  encoding, `hn-node` orchestration, replacing `maybe_sync_forward`) —
  deliberately deferred: the user's own call was to specify this ADR
  now and sequence real implementation relative to real transactions
  actually flowing through consensus (ADR-0037's own still-separate,
  still-open scope), not to build an untestable mechanism today
- a real, non-placeholder `consensus_root`/active-set-at-height
  resolution (ADR-0010, unchanged — named above as a real boundary
  this ADR's own `commit_qc` verification step depends on)
- checkpoint-assisted sync, snapshot-assisted sync, weak subjectivity,
  light-client proofs (ADR-0016's own remaining modes, untouched)
- non-validating full-node mode / pruning policy (ADR-0038, unchanged
  — this ADR assumes every node retains everything)
- `BlockRequestV1`/`BlockResponseV1`'s own stale doc comments and
  whether to wire or retire them (named above, not attempted here)

## Related Specifications

- `docs/adr/ADR-0016-synchronization-checkpoints.md`
- `docs/rfc/consensus/synchronization-checkpoints.md`
