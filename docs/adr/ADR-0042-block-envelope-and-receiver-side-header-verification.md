# ADR-0042: BlockEnvelope And Receiver-Side Header Verification

Status: Accepted

Date: 2026-10-12

Version: 0.1.0

Depends On:

- ADR-0008: Block Format
- ADR-0010: Validator Set Model
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0034: Consensus State Machine Skeleton
- ADR-0037: Multi-Node Consensus Wiring
- ADR-0038: Genesis Format And Node Daemon Bootstrap
- ADR-0039: Devnet Restart Recovery
- ADR-0040: Real Block Hash In `hn-node`

Supersedes: None

Referenced By:

- ADR-0043: Genesis As A Real Block 0 (makes this ADR's own
  `Ctx.chain_position_verified`/`last_block_hash`/`last_timestamp`
  mechanism meaningful from the very first real block: seeded from
  genesis's own real, computed header instead of an arbitrary
  `[0; 32]`/`0` placeholder)
- ADR-0045: Transactions Through Consensus (replaces this ADR's own
  fixed-devnet-value `state_root` check — step 6 of "Decided: Receiver-
  Side Verification Rules," below — with real execution, once blocks
  carry real transactions)

## Context

Three previously-named, closely-related gaps, each already flagged
explicitly in its own earlier ADR:

- ADR-0008's own "Decision" section names a conceptual `BlockEnvelope`
  (`block_version`/`header`/`body`/`justification`) wrapping
  `BlockHeader`/`BlockBody` — but its own "Explicitly not attempted
  here" and Open Decisions both state plainly that no concrete type
  for it exists yet, a "distinct, still-open item of its own."
- ADR-0040's own "Explicitly Not Resolved: Receiver-Side Header
  Reconstruction" states the real gap this ADR's own title names: "A
  receiving node still trusts `ConsensusProposalMessageV1.block_hash`
  exactly as the proposer sends it... a dishonest proposer could still
  claim any `block_hash` it likes." Nothing in `hn-node` has ever
  recomputed or checked a proposer's claimed hash — ADR-0037 introduced
  the field, ADR-0040 made an honest proposer's own claim meaningful,
  neither pass verified it on receipt.
- A closely related, previously-unnamed gap found while designing this
  ADR: `ConsensusProposalMessageV1` itself carries no signature and no
  `proposer` identity field at all — ADR-0038's own "Decided:
  Connection-Layer Simplification" already established that the
  connection layer never attributes a TCP connection to a specific
  validator, so nothing anywhere checked that a `Proposal` message
  actually came from the validator `round_proposer` selects for that
  exact `(height, round)`. Any peer on an established connection could
  broadcast a `Proposal` for the current round with an arbitrary
  `block_hash`, and `hn-node`'s own receive path accepted it purely on
  `(height, round, step)` matching. `ConsensusEngine::handle_proposal`'s
  own doc comment already says proposer eligibility is "the caller's
  job before calling this" — `hn-node`, the caller, never actually did
  it.
- A second, smaller instance of the same class of gap, also found
  while designing this ADR: a `justification` quorum certificate
  embedded in a `Proposal` (used by `hn_consensus::ConsensusState`'s
  own "Locking rule" to let a locked node switch targets) was trusted
  structurally (`target_hash`/`round`/`target_type` fields read
  directly) without ever calling `QuorumCertificate::verify_signatures`
  — unlike a normally-gossiped QC, which already goes through
  `ConsensusEngine::handle_quorum_certificate`'s own
  `qc.verify_signatures(...)` call before being trusted.

All four gaps are instances of one thing: `hn-node`'s receive path for
proposals trusted far more than it verified. This ADR closes all four
together, since they share one mechanism (the proposer signs the whole
header; the receiver recomputes and checks it) rather than needing four
separate fixes.

## Decision

### Decided: `hn_state::BlockEnvelope`

A new, concrete type in `hn-state` (`block_envelope.rs`), closing
ADR-0008's own long-open item exactly as its conceptual sketch named
it:

```rust
pub struct BlockEnvelope {
    pub block_version: u16,
    pub header: BlockHeader,
    pub body: BlockBody,
    pub justification: Option<QuorumCertificate>,
}
```

`block_version` starts at `1` (`BLOCK_VERSION_1`), independent of
`header_version`/`body_version` (ADR-0022's own "Nested Structure
Versions" rule — each already-decided independently). `justification`
is `QuorumCertificate` directly, not a new `BlockJustificationV1`
wrapper: `QuorumCertificate` (ADR-0012) already carries every field
`docs/specs/core/block-format.md` §11's own conceptual
`BlockJustificationV1` sketch names, so it fills that role without a
second, parallel type.

`BlockEnvelope::block_hash()` delegates to `self.header.block_hash()`
(ADR-0040). `BlockEnvelope::verify_body_matches_header()` is a new,
pure, no-external-state method: it recomputes `transactions_root`/
`receipts_root`/`evidence_root`/`extra_data_hash` from `body` via
`BlockBody`'s own existing helper methods and compares each against
the matching `header` field, plus `events_root` against
`list_empty_root()` (no `events` field exists in `BlockBody` at all
yet, HNVM-gated) — every header field *this type alone* can check
without needing the active validator set, this node's own chain
position, or any devnet-wide constant. `header`'s other fields
(`parent_block_hash`, `proposer`, `state_root`, `consensus_root`,
`protocol_parameters_hash`, `timestamp`) are explicitly NOT checked
here — that needs context this type does not have, and is `hn-node`'s
own job (below), the same "assembles already-verified input, does not
re-derive it" boundary this crate already draws elsewhere.

### Decided: `ConsensusProposalMessageV1` Carries A Signed `BlockEnvelope`, Not A Bare Claim

`hn_network::ConsensusProposalMessageV1` is reshaped in place (a
breaking wire change, same precedent ADR-0039 already set for this
exact type):

```rust
pub struct ConsensusProposalMessageV1 {
    pub envelope: BlockEnvelope,
    pub proposer_signature: SignatureEnvelope,
}
```

The old top-level `height`/`round`/`block_hash`/`transactions` fields
are gone: `envelope.header.height`/`.round` already self-describe the
target (ADR-0039's own original reason for adding them survives
unchanged), `block_hash` is derived (`envelope.block_hash()`), never a
second, separately-trusted value that could disagree with the header
it claims to summarize, and `transactions` lives inside `envelope.body`
now. `proposer_signature` is a `SignatureEnvelope` over
`envelope.block_hash()` — not a new, separately-defined signing
payload: `block_hash` is already `HASH_PROFILE_0x0001(
"hnchain.block.header.v1", HNCS(BlockHeader))` (ADR-0008), an
unambiguous, domain-separated 32-byte commitment to the entire header
(including `chain_id`/`network_id`/`height`/`round`/`proposer`), so
signing it directly already satisfies ADR-0002's "No Implicit Signing
Payloads" rule without inventing a wrapper struct.

### Decided: Receiver-Side Verification Rules

`hn-node`'s new `verify_proposal` function (`node.rs`) runs before any
incoming `Proposal` is ever handed to `ConsensusEngine::
handle_proposal`:

1. Recompute `block_hash = envelope.block_hash()` — there is no
   separately-carried claimed value left to trust or distrust.
2. `envelope.header.chain_id`/`.network_id` must match this node's own.
3. `round_proposer(ordered_ids, header.height, header.round)` must
   equal `header.proposer` exactly — the claimed proposer must be the
   one this exact slot actually selects, not merely some registered
   validator.
4. `proposer_signature` must verify against `header.proposer`'s own
   registered consensus key (`hn_state::active_key`, the same
   resolution `ConsensusVote::verify` already uses) over `block_hash`.
5. `envelope.verify_body_matches_header()` must succeed.
6. `header.state_root`/`.consensus_root`/`.protocol_parameters_hash`
   must each match this devnet's own fixed values (`ctx.state_root`,
   `DEVNET_VSC`, `protocol_parameters_placeholder_hash()`) — the same
   values an honest proposer already computes (ADR-0040), now actually
   compared rather than assumed. **The fixed-value check against
   `state_root` specifically is replaced by ADR-0045** ("Transactions
   Through Consensus"): once blocks carry real transactions, a
   validator recomputes `state_root` by actually executing the block
   rather than comparing it to a devnet-wide constant.
   `consensus_root`/`protocol_parameters_hash` remain fixed-value
   checks, unchanged (ADR-0010's own still-open item).
7. If present, `envelope.justification` must pass
   `QuorumCertificate::verify_signatures` — the same check a normally
   gossiped QC already gets via `handle_quorum_certificate`, now also
   applied to one arriving embedded in a `Proposal`.

A failure at any step is a dropped, logged message (`REJECTED proposal
height=... round=... reason="..."`), never a propagated error: this is
externally-triggerable network input, and no externally-triggerable
input may crash this process — the same principle ADR-0038's own
"no-panic discipline" already applies at the process boundary, now
applied one layer in, at the message-verification boundary. (Decoding
`envelope.payload` itself can still propagate a fatal error on
malformed bytes, same as every other message type already does in
`handle_peer_message` today — a real, separate, broader
decode-resilience gap this ADR does not attempt to close; see
"Explicitly Not Resolved," below.)

### Decided: Chain-Position-Gated Checks

`parent_block_hash` and timestamp monotonicity are checked only when
this node's own chain position is actually known to be correct — a
new `Ctx.chain_position_verified: bool`, paired with `Ctx.
last_timestamp: UnixTimeMillis` alongside the already-existing `Ctx.
last_block_hash`. A node that has never needed `maybe_sync_forward`'s
passive catch-up jump (ADR-0039) has these two values trustworthy for
its current height by construction; the jump itself sets
`chain_position_verified = false` (the jumped-to height's real parent
is unknown — real block/state sync, ADR-0016, remains out of scope),
and a successful local finalize sets it back to `true` (from that
point on, `last_block_hash`/`last_timestamp` are correct again).
`verify_proposal` skips exactly these two checks while `false` —
every other check needs no chain-position context at all and always
runs.

When known, the rule is **strict timestamp monotonicity against the
parent's own timestamp** (`header.timestamp > last_timestamp`) — no
wall-clock/drift bound against this node's own local clock. This was a
genuine fork (ask the user, don't invent a new protocol constant): a
concrete bound would need a real magnitude decided somewhere, and the
alternative of leaving `timestamp` entirely unchecked would silently
reintroduce exactly the kind of "accepted as given" gap this ADR exists
to close. Monotonicity-only needs no new number at all and already
rules out the one thing that actually matters for this devnet's own
ordering guarantee — a proposer backdating or repeating a timestamp.

A new `Ctx.pending_headers: HashMap<Digest, BlockHeader>` tracks each
cached proposal's own full header (mirroring `ConsensusEngine`'s own
internal `proposed_blocks` cache, which only has `transactions` — no
field for `timestamp`), populated at both call sites that call
`ConsensusEngine::handle_proposal` (self-propose and
verified-receive), and cleared the same way `commit_finalized_block`
clears `proposed_blocks` on a successful finalize: the just-finalized
header's own `timestamp` becomes the new floor, every other cached
header (competing, now-irrelevant rounds at the same height) is
dropped.

## Rejected Options

### A Separate Signing-Payload Wrapper For `proposer_signature`

Rejected: `block_hash` is already a real, domain-separated,
unambiguous 32-byte digest of the entire header (ADR-0008). Wrapping
it in a second struct just to sign it would duplicate what `block_hash`
already is, for no additional binding `block_hash` doesn't already
provide.

### A Numeric Wall-Clock Timestamp Bound

Rejected (the user's own call, not this ADR's default): would require
inventing a concrete magnitude (seconds? milliseconds? some multiple of
`base_timeout_ms`?) with no existing basis to derive it from — the same
"never invent a placeholder economic/protocol value" constraint this
project already applies to monetary parameters, extended here to a new
consensus-timing constant. Monotonicity-only closes the real misuse
case (a repeated or backdated timestamp) without that invention.

### Fixing General Malformed-Message Decode Resilience In This Same Pass

Rejected: every message type in `handle_peer_message` already
propagates a fatal error out of a failed `::decode` call today, not
just `ConsensusProposalMessageV1` — a real, pre-existing gap, but a
different, broader one than "is this specific, successfully-decoded
proposal legitimate." Fixing it here would blur two genuinely separate
hardening tasks into one pass.

### Wiring Real Re-Proposal `justification` On The Proposing Side

Rejected: `hn-node`'s own proposer has always sent `justification:
None` unconditionally, even though `ConsensusState`'s own "Locking
rule" supports a real one. This ADR verifies a `justification` when
one arrives; it does not change when `hn-node` itself sends one — a
distinct, separate gap, unrelated to receiver-side trust.

## Compatibility

Breaking change to `ConsensusProposalMessageV1`'s own wire shape —
acceptable pre-mainnet, consistent with every prior breaking change
this project has already made to devnet-only wire types. No change to
`BlockHeader`/`BlockBody`/`hn_consensus::ConsensusEngine`'s own public
surface.

## Security Considerations

Closed by this ADR:

- A dishonest proposer claiming a `block_hash` unrelated to any real
  header (ADR-0040's own named risk) — now cryptographically
  impossible to get accepted, since the hash is derived, not claimed.
- Any connected peer forging a `Proposal` for the current round — now
  rejected unless signed by the exact validator `round_proposer`
  selects, using that validator's own registered key.
- A forged `justification` QC with fabricated `target_hash`/`round`
  fields and no real signatures, used to trick a locked node into
  unlocking — now rejected by the same `verify_signatures` check a
  normal QC already got.

Explicitly not closed, named rather than silently assumed:

- General malformed-message decode resilience (see "Rejected Options,"
  above) — any message type, including this one, can still crash the
  process on a failed `::decode` before `verify_proposal` ever runs.
- `parent_block_hash`/timestamp checks are skipped during passive
  catch-up (`chain_position_verified == false`) — a node that just
  jumped forward cannot detect a wrong parent or a backdated timestamp
  until it finalizes a block itself. Real block/state sync (ADR-0016)
  is the only real fix; this ADR does not attempt it.
- No wall-clock bound on `timestamp` — a proposer can still claim any
  timestamp strictly after the parent's own, including one far in the
  future. Named, not invented around (see "Rejected Options," above).

## Open Decisions

- the document-commitment-style "which files" question this ADR does
  not touch at all (unrelated, ADR-0041, unchanged)
- general malformed-message decode resilience across every message
  type (unchanged, newly named explicitly here)
- real re-proposal `justification` wiring on `hn-node`'s own proposing
  side (unchanged, newly named explicitly here)
- a real wall-clock timestamp bound, if one is ever wanted (explicitly
  rejected this pass, not merely deferred by oversight)
- real block/state sync — **specified, not yet implemented, ADR-0044**
  ("Full-Verification Block And State Sync") — the only real fix for
  chain-position-gated checks being skipped during catch-up

## Related Specifications

- `docs/specs/core/block-format.md`
