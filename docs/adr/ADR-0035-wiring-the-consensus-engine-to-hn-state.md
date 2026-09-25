# ADR-0035: Wiring The Consensus Engine To `hn-state`

Status: Proposed

Date: 2026-09-25

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0019: Storage / State Interfaces
- ADR-0030: Transaction And Block Application (V1 Scope)
- ADR-0031: Write-Set Value Bytes And Overlay State Reader
- ADR-0033: Atomic Write-Set Commit (`StateCommitter`)
- ADR-0034: Consensus State Machine Skeleton

Supersedes: None

## Context

ADR-0034's `ConsensusState` is deliberately pure: `apply` trusts every
event completely, never touching `hn-state` at all. That was the right
scope for the round transition table itself, but it also means nothing
in this codebase yet connects a real vote or quorum certificate to
cryptographic verification, connects a finalized block to actually
applying its transactions, or determines which validators are even
allowed to vote in the first place. `hn-state` already has everything
needed for all three:
[`ConsensusVote::verify`]/[`QuorumCertificate::verify_signatures`]
(ADR-0012) check signatures; [`hn_state::active_set`] (ADR-0010)
derives an epoch's voting membership from an already-fetched candidate
slice; [`apply_and_commit_block`] (ADR-0030/ADR-0031/ADR-0033) applies
a block's transactions and commits the result atomically to a real
backend. Nothing has ever called any of them together.

## Decision

**`ConsensusEngine`**, new in `hn-consensus`, wraps `ConsensusState`
with exactly the composition needed to make those three connections
real, while keeping the pure state machine itself completely unchanged:

```rust
pub struct ConsensusEngine {
    state: ConsensusState,
    proposed_blocks: HashMap<Digest, Vec<TransactionEnvelope>>,
}
```

**`proposed_blocks`: a transient cache, not block storage.** Committing
a finalized block needs its actual transaction list, but
`ConsensusAction::Finalized` only carries a `block_hash` — there is no
`BlockStore` (ADR-0019's own still-undefined interface) anywhere in
this codebase, and none is built here. Instead, `handle_proposal`
caches the transactions it was given, keyed by `block_hash`, exactly
because the engine already has to receive them to vote on them in the
first place; `commit_finalized_block` looks them up when (and only
when) a block actually finalizes, and every other cached entry for that
height is discarded once it does. This is the "block header/receipts
storage" question surfacing exactly where the user predicted it would —
resolved here as narrowly as possible (an in-memory transaction cache,
nothing durable, no header, no receipts retention), not by building
`BlockStore` or a concrete `BlockHeader` type, both real, separate,
larger decisions left for when a caller actually needs them.

**Vote/certificate handling composes existing primitives, adds no new
cryptography or quorum logic:**

- `handle_quorum_certificate(qc, reader, ordered_active_set)` calls
  `QuorumCertificate::verify_signatures` first
  (`ConsensusError::QuorumCertificateInvalid` on failure), then
  dispatches to `ConsensusEvent::PrevoteQuorum`/`PrecommitQuorum` by
  `qc.certificate_type` and feeds it into `ConsensusState::apply`
  unchanged.
- `verify_vote(vote, reader, ordered_active_set)` calls
  `ConsensusVote::verify` (signature), then `is_eligible_signer`
  (`ordered_active_set` membership plus the signer's live status via
  `fetch_validator_record`). It does **not** aggregate votes into a
  `QuorumCertificate` — no vote-pool/aggregation algorithm (collecting
  individual votes, building a `signer_commitment` bitmap, detecting
  the 2f+1 threshold) exists anywhere in this codebase, and building
  one is real, separate work this ADR does not take on. This method
  answers exactly one question — "is this one vote legitimate" — the
  input a future aggregator would consume.

**`ordered_active_set` is always a parameter, never computed by this
crate.** It is exactly `hn_state::active_set`'s own output for the
relevant epoch, mapped to each record's `validator_id` — `active_set`
itself already takes an already-fetched candidate slice rather than
querying storage (this crate cannot enumerate storage either, the same
structural fact ADR-0032 already established for `chamber_weights`),
and it is computed once per epoch, not once per vote or certificate, so
recomputing it inside a per-event method would be the wrong place for
it regardless of enumeration.

**`commit_finalized_block` is the first real caller of
`apply_and_commit_block` from outside `hn-state`/`hn-storage`'s own
tests.** Requires the engine's own step to be `Finalize`
(`ConsensusError::HeightNotFinalized` otherwise — committing needs the
height that just finalized, only well-defined at that exact step) and
the requested `block_hash` to have a cached transaction list
(`ConsensusError::UnknownProposedBlock` otherwise). `validator_candidates`
is threaded straight through to `apply_and_commit_block`, unchanged —
this crate cannot supply it any more than `hn-state` itself can (same
enumeration gap).

## Explicitly Not Resolved

**No vote aggregation / `QuorumCertificate` construction.** `verify_vote`
checks one vote at a time; nothing here collects votes toward a
threshold or builds a `signer_commitment`. A `QuorumCertificate` must
still arrive from somewhere else (a test, or eventually real network
gossip plus a real aggregator) before `handle_quorum_certificate` can
do anything with it.

**No real block/header storage.** `proposed_blocks` is explicitly not
`BlockStore` — see "Decision," above. A real `BlockHeader` type, real
durable block-body retention, and real receipts storage remain
undecided, exactly where this pass stopped rather than guessing ahead
of a real need.

**No leader election, networking, or timers.** Unchanged from
ADR-0034's own scope — `handle_proposal` still does not check proposer
eligibility (ADR-0011), and nothing here drives the engine from real
network messages or real timeouts.

**No epoch-transition handling.** `ordered_active_set` is a parameter
the caller must recompute and pass in whenever the epoch actually
changes (ADR-0010, "Epoch Boundaries") — this crate has no opinion on
when that is.

## Rejected Options

### Building A Vote Pool / QC Aggregator In This Pass

Rejected: a real threshold-crossing aggregator (collecting individual
votes, deduplicating signers, building a canonical `signer_commitment`
bit order, detecting when `2f+1` is reached) is a genuinely separate
algorithm from "verify one vote" or "react to an already-formed QC" —
conflating it into this pass would have been exactly the kind of
undersized, half-finished addition this project avoids. `verify_vote`
is scoped to produce exactly the input such an aggregator would need,
without being one.

### A Real `BlockStore`/`BlockHeader` Type Instead Of An In-Memory Cache

Considered, per the user's own framing that this would "naturally
arise" here. Rejected for this pass specifically because it is a much
larger, separate decision (durability, retention policy, header field
finalization, interaction with `StateCommitter`'s own still-open
"Atomic State Commit" scope, ADR-0033) than what
`commit_finalized_block` actually needs to function — an in-memory
transaction cache closes the gap completely for this crate's own
purposes without deciding any of that.

### Making `ConsensusEngine` Generic Over A Stored `StateReader`/`StateCommitter`

Considered holding a store handle on the engine itself (avoiding
passing `reader`/`store` to every method). Rejected: every other
`hn-state` "composing" function in this session (`apply_transaction`,
`apply_block`, `apply_and_commit_block` itself) takes its store as a
parameter rather than owning one, and a long-lived engine holding a
generic store would need to be generic everywhere, including in tests
that do not need real storage at all — passing the store explicitly to
only the two methods that need it (`handle_quorum_certificate`,
`verify_vote`, `commit_finalized_block`) keeps every other method
storage-agnostic.

## Security Considerations

Untrusted proposal transactions:

- Risk: `handle_proposal` caches and later commits whatever transaction
  list it is given, with no check that it actually matches
  `block_hash` or came from the round's real proposer.
- Mitigation: none in this crate — explicitly the caller's job (ADR-0011
  proposer eligibility, and any future block-hash-matches-body check),
  the same boundary `ConsensusState::apply` itself already draws for
  proposals.

Vote/certificate trust boundary:

- Risk: a caller that skips `verify_vote`/`handle_quorum_certificate`
  and feeds `ConsensusState::apply` a raw, unverified `QuorumCertificate`
  event directly bypasses all the checking this ADR adds.
- Mitigation: `ConsensusState` itself remains intentionally unaware of
  verification (ADR-0034's own boundary) — `ConsensusEngine` is the
  layer meant to prevent this, but nothing stops a future caller from
  constructing events directly instead of going through it. Named here
  as a real caller-discipline requirement, not enforced by the type
  system.

## Compatibility

Purely additive: one new type (`ConsensusEngine`), new `ConsensusError`
variants, one new dev-dependency (`hn-storage`, test-only). No changes
to `ConsensusState`/`ConsensusStep`/`ConsensusEvent`/`ConsensusAction`
or any `hn-state` type's public signature.

## Open Decisions

- vote-pool / `QuorumCertificate` aggregation algorithm (see
  "Explicitly Not Resolved")
- `BlockStore`/concrete `BlockHeader` type and real receipts storage
  (see "Explicitly Not Resolved")
- leader-election integration, networking, real timers (ADR-0034's own
  still-open items, unchanged)
- epoch-transition-driven `ordered_active_set` recomputation

## Related Specifications

- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0019-storage-state-interfaces.md`
- `docs/adr/ADR-0030-transaction-and-block-application.md`
- `docs/adr/ADR-0033-atomic-write-set-commit.md`
- `docs/adr/ADR-0034-consensus-state-machine-skeleton.md`
