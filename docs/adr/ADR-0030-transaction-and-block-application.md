# ADR-0030: Transaction And Block Application (V1 Scope)

Status: Proposed

Date: 2026-09-24

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0008: Block Format
- ADR-0019: Storage / State Interfaces
- ADR-0026: Threshold And Multisignature Authorization
- ADR-0027: Identity State And Account Key Bootstrap

Supersedes: None

## Context

Every commit across ADR-0025 through ADR-0029 ends with the same
sentence, in one phrasing or another: "no block-processing pipeline
exists anywhere in this codebase to call it yet." `hn-state` now has a
real, tested primitive for nearly everything a transaction can do
(`apply_transfer`, `apply_stake`/`apply_unstake`,
`apply_validator_update`, `apply_propose`/`apply_vote`,
`apply_permission_update`, `apply_identity_bootstrap`/
`apply_identity_rotation`) and a real, tested composing verifier
(`TransactionEnvelope::verify`) — but nothing has ever called more than
one of them together, on an actual sequence of transactions, the way a
real block would require.

This ADR decides the scope of the first thing that does — deliberately
narrow, not a claim that `hn-consensus`/`hn-node` start today. BFT
voting, leader election, networking, and a real durable storage backend
(`hn-storage`'s own "initial storage backend," ADR-0019, still open)
are all separate, much larger undertakings this ADR does not begin.
What it decides is the **state-transition function** a future consensus
layer will eventually call once it has agreed on a block's contents —
useful, testable, and callable on its own well before that layer exists,
the same way every `apply_*` primitive already was.

## Decision

**Scope: `hn-state`, not a new crate.** `apply_transaction`/
`apply_block` live in `hn-state` alongside every primitive they call,
not in `hn-consensus` (still a pure stub) or a new crate. Nothing about
block application needs consensus-specific knowledge (voting, leader
election) — it only needs "here is an ordered list of transactions,
apply them" — and `hn-consensus` already depends on `hn-state`, never
the reverse, so this is the natural, dependency-respecting location:
consensus will *call* this function once it exists, not own it.

**Decided: `apply_transaction` composes what already exists, adds
nonce and validity-window checks, and returns a write-set — it does not
write to a `StateWriter`.**

```text
apply_transaction(envelope, reader: &impl StateReader, current_height)
  -> StateResult<AppliedTransaction>

AppliedTransaction
  tx_id
  write_set: Vec<Leaf>
  receipt: ReceiptV1
```

1. `envelope.verify(reader)` (ADR-0026/ADR-0027) — a hard error, not a
   receipt: an unauthorized transaction was never validly included.
2. Nonce check: `sender`'s stored nonce (absent means
   `AccountNonce::INITIAL`, the same "absence means the default"
   convention every other section already uses) must equal
   `envelope.nonce` exactly (ADR-0006, "Nonce" — strictly increasing,
   gap-free) — a hard error on mismatch
   (`StateError::NonceMismatch`), the same inclusion-precondition class
   as a bad signature, not a legitimate execution outcome.
3. Validity window check (ADR-0006) against `current_height` — a hard
   error (`StateError::TransactionOutsideValidityWindow`) if outside
   `[min_height, max_height]`, same class again.
4. `decode_transaction_payload` (ADR-0027) — a hard error
   (`StateError::UndecidedTransactionPayload` or a decode failure) for
   a malformed payload or one of the 3 still-undecided `tx_type`s.
5. Dispatch to the matching `apply_*_with_receipt`, fetching whatever
   pre-state that operation needs via new, small `fetch_*` helpers
   (below) — mirroring `fetch_identity`/`fetch_validator_record`'s own
   shape. A legitimate execution failure (insufficient balance, wrong
   validator status, ...) becomes a `Failed` receipt here, exactly as
   each `apply_*_with_receipt` already decided — `apply_transaction`
   does not reinterpret that.
6. The nonce leaf updates unconditionally, appended to the write-set
   regardless of whether step 5 succeeded or produced a `Failed` receipt
   — ADR-0006's own "nonce still consumed on failure" rule, implemented
   for the first time.

**Decided: no fee deduction in this pass.** The fixed fee rate/floor is
still an undecided economic parameter (ADR-0023's own Open Decisions) —
inventing one here would violate this project's standing "no implicit
economic values" rule. `apply_transaction` does not deduct any fee,
named explicitly as a known v1 gap, not silently skipped.

**Decided: `apply_block` aggregates, does not attempt the full new
`state_root`.**

```text
apply_block(transactions: &[TransactionEnvelope], reader, current_height)
  -> StateResult<BlockApplicationResult>

BlockApplicationResult
  applied: Vec<AppliedTransaction>
  transactions_root
  receipts_root
```

Applies each transaction via `apply_transaction` (against the same
pre-block `reader` — see "Explicitly Not Resolved" for why), collects
each `tx_id` and `ReceiptV1.digest()`, and computes `transactions_root`/
`receipts_root` via the already-implemented `list_merkle_root`
(ADR-0008). It does **not** attempt to compute `BlockHeader`'s own
`state_root`: `compute_state_root` (ADR-0007) needs the *complete*
current leaf set to produce a meaningful root, not a block's write-set
delta alone, and nothing in this project yet maintains that complete
set anywhere (no durable backend exists, ADR-0019's own still-open
item). Producing a real `state_root` is a real storage-backend
dependency this ADR does not take on.

**Decided: `fetch_nonce`, `fetch_balance`, `fetch_asset`,
`fetch_transfer_party`, `nonce_leaf`.** New, small `StateReader`-based
fetch helpers, each mirroring `fetch_identity`/`fetch_validator_record`'s
established shape (absence maps to the section's own already-decided
default: `AccountNonce::INITIAL`, `native_balance = 0`, empty
`holdings`) — no new design questions, just the same pattern applied to
the three sections nothing had a fetch helper for yet.
`fetch_transfer_party` composes the two asset-adjacent fetches into the
`TransferParty` shape `apply_transfer` already expects.

## Explicitly Not Resolved

**Intra-block same-sender visibility.** `apply_block` applies every
transaction against the *same* pre-block `reader` — a second
transaction from a sender whose nonce or balance the *first* transaction
in the same block already changed will not see that change; both would
be checked against the pre-block state. A correct implementation needs
an overlay/staging reader layering "this block's writes so far" on top
of the base `reader`, which in turn needs every `apply_*` function to
expose canonical *value bytes*, not just `(state_key, leaf_hash)` pairs
— those functions were built for state-root computation, not for
reconstructing storable values, and changing that return shape touches
every transition primitive built this session. Two same-sender
transactions in one block will, at best, both apply against stale state
today (silently producing a block whose write-set is likely wrong for
the second one) — named honestly here as a real, known v1 limitation,
not fixed by this pass. A duplicate `state_key` across two colliding
writes is at least loudly rejected by `compute_state_root` if anyone
tries to use this write-set for that purpose, rather than silently
merged wrong.

**`governance` (`propose`/`vote`) stays unwired.** `apply_propose`/
`apply_vote` need chamber-weight totals as caller-supplied parameters
(ADR-0025) — computing them requires summing every validator's
`bonded_stake`/counting `Active` validators, a query this crate has
never built (`governance_transition.rs`'s own documentation already
named this: "this crate owns state transitions, not the query that
produces a total validator count or a total bonded-stake sum"). Wiring
`governance` needs that query first — real, separate, smaller follow-up
work, not done here.

**`StateWriter` stays uncalled.** Nothing in this codebase implements
it yet (no durable backend, ADR-0019), and `apply_transaction`/
`apply_block` do not need it given they return a write-set rather than
persisting one. Wiring an actual persistence step is a distinct future
decision, gated on a real backend existing at all.

**Real `BlockHeader`/block validation** (previous block hash linkage,
proposer/signature checks, `consensus_root` agreement, timestamp rules)
is entirely out of scope — this ADR only applies a block's *transaction
list*, it does not validate that the list came from a legitimately
agreed-upon block.

## Rejected Options

### Building An Overlay `StateReader` Now, To Solve Intra-Block Visibility

Rejected for this pass: would require every existing `apply_*` function
to also expose canonical value bytes (not just leaf hashes), a change
touching every transition primitive built across ADR-0025 through
ADR-0029. Named as the real fix in "Explicitly Not Resolved" rather than
attempted piecemeal here.

### Computing `state_root` From The Block's Write-Set Alone

Rejected: `compute_state_root` is defined over a *complete* leaf set
(ADR-0007); passing it a block's delta would silently produce a root
that only reflects this block's touched leaves, not the real
post-block world state — wrong, not just incomplete.

### Deducting A Placeholder Fee

Rejected outright, per this project's own standing rule: no economic
value — including an implicit or "obviously temporary" placeholder — may
be encoded before it is accepted in ADR-0023.

## Security Considerations

Intra-block replay via duplicate nonce:

- Risk: two transactions from the same sender in one block, both
  carrying the same (correct, pre-block) nonce.
- Mitigation: both would be individually valid against the shared
  pre-block `reader` (the exact limitation named above) — not
  mitigated by this ADR. A real pipeline needs the overlay-reader fix
  before this is safe to run against untrusted block proposers.

Missing fee deduction:

- Risk: applying a block without deducting fees means a v1 caller of
  this pipeline cannot yet observe real economic cost, or reject a
  transaction for insufficient fee coverage.
- Mitigation: none — an accepted, explicitly named gap pending
  ADR-0023's own fee-amount decision, not a security flaw this ADR
  silently introduces.

## Compatibility

Purely additive: two new functions and four new small fetch helpers in
`hn-state`, no changes to any existing type's wire encoding or public
signature.

## Open Decisions

- intra-block overlay reader / value-byte-exposing `apply_*` return
  shape (see "Explicitly Not Resolved")
- governance chamber-weight-total query (see "Explicitly Not Resolved")
- real backend-integrated `StateWriter` persistence and `state_root`
  computation (ADR-0019's own still-open "initial storage backend")
- real block/header validation (proposer, signatures, `consensus_root`
  agreement, timestamp) — the actual consensus layer's job, not this
  ADR's

## Related Specifications

- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0019-storage-state-interfaces.md`
- `docs/adr/ADR-0025-governance-model.md`
