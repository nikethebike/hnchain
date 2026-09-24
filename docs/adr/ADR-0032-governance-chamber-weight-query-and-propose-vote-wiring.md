# ADR-0032: Governance Chamber-Weight Query And Propose/Vote Wiring

Status: Proposed

Date: 2026-09-24

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0006: Transaction Format
- ADR-0010: Validator Set Model
- ADR-0019: Storage / State Interfaces
- ADR-0023: Tokenomics And Economic Model
- ADR-0025: Governance Model
- ADR-0030: Transaction And Block Application (V1 Scope)
- ADR-0031: Write-Set Value Bytes And Overlay State Reader

Supersedes: None

## Context

ADR-0030 rejected `governance` outright with
`StateError::UndecidedTransactionPayload`, naming the reason in its own
"Explicitly Not Resolved" section: `apply_propose`/`apply_vote` need a
`validator_chamber_total_weight`/`staker_chamber_total_weight` snapshot
as caller-supplied parameters (ADR-0025, "Decided: Chambers,
Membership, And Weight" — one vote per `Active` validator for the
validator chamber, the sum of every validator's own `bonded_stake` for
the staker chamber, regardless of status), and nothing in this crate
ever computed that sum. `governance_transition.rs`'s own documentation
had already named the same gap even earlier: "this crate owns state
transitions, not the query that produces a total validator count or a
total bonded-stake sum."

Closing this looked at first like "just write the missing query
function" — but a real structural fact surfaced before writing any
code: [`crate::StateReader`] (ADR-0019) is a point-lookup interface
only (`get(state_key) -> Option<Vec<u8>>`); both actual backends in
`hn-storage` (in-memory, `redb`) implement nothing beyond
`get`/`set`. There is no way to enumerate "every validator record" from
a `StateReader` alone anywhere in this codebase. This is not a new
problem invented by governance — `active_set` (ADR-0010) already hit
the identical wall deriving an epoch's active set, and already resolved
it the same way this ADR does: take an already-fetched candidate slice
as a parameter, rather than have this crate try to query storage
itself (its own charter, `lib.rs`: "owns protocol state interfaces
without binding them to a concrete storage engine").

## Decision

**`ChamberWeights`/`chamber_weights(candidates: &[ValidatorRecordV1])`**,
new in `governance_transition.rs`: a pure function computing
`validator_chamber_total_weight` (count of `status == Active` records)
and `staker_chamber_total_weight` (checked-arithmetic sum of every
record's own `bonded_stake`, regardless of status — reuses
`StateError::GovernanceTallyOverflow`, the same variant this module's
own vote-tally arithmetic already uses) — mirrors `active_set`'s own
"caller already fetched it" signature shape exactly.

**`fetch_proposal`/`fetch_proposal_vote`**, new in
`governance_transition.rs`: ordinary `StateReader`-based point-lookup
fetch helpers (a proposal by its own `proposal_id`, a vote by
`(proposal_id, voter)`) — no enumeration needed here, since both are
already-known single keys, the same class of lookup
`fetch_identity`/`fetch_validator_record` already are.

**`GOVERNANCE_VOTING_WINDOW`/`GOVERNANCE_QUORUM_NUMERATOR`/
`GOVERNANCE_QUORUM_DENOMINATOR`** are now real `pub const` values in
`governance_transition.rs` (`302_400`, `1`, `5` — ADR-0023's own
already-decided figures), not just documentation prose: `apply_propose`
and `finalize_proposal` still take them as parameters rather than
reading the constants directly (so both stay testable against other
values), but `apply_transaction` — this module's first real caller — is
what finally makes a canonical source worth defining, the same
`MINIMUM_VALIDATOR_BOND`/`UNBONDING_PERIOD_BLOCKS` pattern
`validator_transition.rs` already established once ADR-0023 resolved
those values.

**`apply_transaction`/`apply_block` gain a
`validator_candidates: &[ValidatorRecordV1]` parameter** (required, not
optional — asked the user, this was the one genuine fork in this pass:
a required parameter mirroring `active_set`'s own precedent exactly,
vs. an `Option`, vs. leaving `governance` unwired until a real
enumeration capability exists elsewhere; required-parameter won).
Unused by every payload except `Propose`. `apply_block` passes the same
slice, unchanged, to every transaction in the block — a `Propose`'s
chamber-weight snapshot reflects validator state as of the block's own
`current_height`, not any earlier transaction in the same block. This
is not a shortfall: ADR-0025 already specifies "snapshotted at the
proposal's creation height," a height-granularity commitment, not a
sub-block transaction-ordinal one — a single per-block snapshot matches
that discipline exactly, the same reasoning that motivated
epoch-snapshotting the active set in the first place ("to avoid a
moving quorum target").

**`governance` is now dispatched in `apply_transaction`**:

- `Propose`: fetches `sender`'s own `ValidatorRecordV1`
  ([`StateError::UnknownValidator`] if absent — the same hard-error
  treatment `stake`/`unstake` already give a missing record, not a
  `Failed` receipt), computes `chamber_weights(validator_candidates)`,
  and calls `apply_propose_with_receipt` with the live snapshot and
  `GOVERNANCE_VOTING_WINDOW`.
- `Vote`: fetches the target `ProposalRecordV1` by the payload's own
  `proposal_id` ([`StateError::UnknownProposal`] if absent — same
  "caller-supplied key not found" hard-error class as
  `UnknownValidator`, distinct from `apply_vote`'s own internal
  `UnknownProposal` check for a proposal-id *mismatch* once a record
  *was* found), `sender`'s own optional validator record, and any
  existing vote already cast.

**`governance_transition.rs`'s `Leaf`-returning functions are converted
to `Write`** (ADR-0031's own predicted follow-up, now due):
`ProposeOutcome = (Digest, [Write; 1])`, `apply_vote -> [Write; 2]`,
`finalize_proposal -> Option<[Write; 1]>`. `finalize_proposal` itself
stays unwired (see "Explicitly Not Resolved") but shares the same
`proposal_record_write` helper `apply_propose`/`apply_vote` use, so it
had to convert regardless of its own wiring status.

## Explicitly Not Resolved

**`finalize_proposal` still has no caller.** Closing a proposal once
its voting window passes needs a per-block sweep over every
currently-`Voting` proposal — the same missing "enumerate everything of
a kind" capability `chamber_weights` needed a caller-supplied slice to
work around, except here there is no natural "caller already knows the
single key" shortcut the way `Propose`/`Vote` had (a validator's own
record, a specific `proposal_id`): closing proposals needs to discover
*which* proposals are open at all. Left as `governance_transition.rs`'s
own already-honest "periodic sweep, no caller yet" situation, unchanged
by this ADR.

**No proposal-enumeration index of any kind is introduced.** A future
fix (for `finalize_proposal`, or for anything else needing "every open
proposal") could extend `StateReader` itself, build a dedicated
secondary index in the `governance` domain, or take an enumerated slice
the same way `chamber_weights` does — deliberately not decided here;
each has real trade-offs (interface scope, storage cost, caller
burden) this ADR did not evaluate.

## Rejected Options

### `Option<&[ValidatorRecordV1]>` Instead Of A Required Slice

Considered: friendlier for non-governance callers (no need to supply
an unused empty slice). Rejected because it trades a type-enforced
precondition for a runtime-checked one, and the asymmetry (required for
`Propose`, ignored otherwise) is already visible in the function's own
documentation — an empty slice for a governance-free block costs
nothing at the call site.

### Extending `StateReader` With An Enumeration Method

Rejected: ADR-0019 deliberately scoped `StateReader`/`StateWriter` to
exactly the two interfaces with a real caller, leaving the other seven
conceptual ADR-0019 interfaces undefined until one exists — adding
enumeration now would be exactly the "speculative architecture ahead of
actual need" that document's own reasoning already rejects doing
elsewhere in this crate, and is a big enough interface-design question
to deserve its own dedicated ADR if it's ever taken on, not a
side-effect of wiring governance.

### Recomputing `chamber_weights` Per-Transaction From The Overlay Reader

Rejected as impossible, not merely undesirable: [`OverlayReader`]
(ADR-0031) only tracks specific `state_key -> value` writes it has
already seen; it cannot answer "list every validator," so there is no
way to derive a fresher `validator_candidates` slice mid-block without
the same enumeration capability this ADR already established doesn't
exist. A single pre-block snapshot is what ADR-0025's own
height-granularity language actually calls for regardless (see
"Decided," above).

## Security Considerations

Stale chamber-weight snapshot within a block:

- Risk: a `Propose` transaction's chamber-weight snapshot does not
  reflect an earlier same-block `stake`/`unstake`/`validator_update`
  transaction's effect on a *different* validator's `bonded_stake` or
  status.
- Mitigation: not a security gap — ADR-0025 defines the snapshot at
  height granularity, not sub-block-ordinal granularity; a caller
  supplying `validator_candidates` as of the block's own start height
  is exactly the specified behavior, not an approximation of it.

Missing proposal-close enforcement:

- Risk: `finalize_proposal` having no caller means no proposal ever
  actually transitions out of `Voting` in this codebase yet, even past
  its voting window.
- Mitigation: none — an accepted, explicitly named gap (see "Explicitly
  Not Resolved"), not a security flaw specific to this ADR's own scope;
  `apply_vote` itself still correctly rejects a vote cast after
  `voting_ends_at_height` regardless of whether finalization has run.

## Compatibility

Breaking within `hn-state` only (no external crate depends on the
changed signatures yet — `hn-consensus`/`hn-node` are still stubs):
`apply_transaction`/`apply_block` gain a new required parameter,
`governance_transition.rs`'s write-producing functions change from
`Leaf`-shaped to `Write`-shaped return types (mirroring ADR-0031). No
wire-format/encoding change anywhere.

## Open Decisions

- proposal-enumeration mechanism for `finalize_proposal` (see
  "Explicitly Not Resolved")
- every other governance Open Decision ADR-0025 itself still lists
  unchanged (vote-changing, execution scope, delegation's own tracking
  mechanism, proposal cancellation, proposal spacing/rate limiting)

## Related Specifications

- `docs/adr/ADR-0010-validator-set-model.md` (`active_set`'s own
  candidate-slice precedent)
- `docs/adr/ADR-0019-storage-state-interfaces.md`
- `docs/adr/ADR-0023-tokenomics-and-economic-model.md`
- `docs/adr/ADR-0025-governance-model.md`
- `docs/adr/ADR-0030-transaction-and-block-application.md`
- `docs/adr/ADR-0031-write-set-values-and-overlay-state-reader.md`
