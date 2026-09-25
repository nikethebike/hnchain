# ADR-0034: Consensus State Machine Skeleton

Status: Proposed

Date: 2026-09-25

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0013: Finality Rules

Supersedes: None

## Context

`hn-consensus` has been a pure stub since this project's crate skeleton
was first laid out — a crate-level doc comment ("must not perform
network I/O directly and must not rely on node process lifecycle
behavior") and nothing else. ADR-0009 already decided the family
(Tendermint-style BFT), the round structure (`propose -> prevote ->
precommit`, each with its own timeout), and the view-change mechanism
(a round's own absence of a qualifying `precommit` quorum before
timeout is itself sufficient justification to advance — no separate
timeout-certificate object). ADR-0012 already gives this crate real,
concrete, oracle-verified types to build on: `ConsensusVote`,
`QuorumCertificate` (with a fixed quorum-threshold formula,
`signed_voting_power * 3 > total_voting_power * 2`), `VoteType`
(`prevote`/`precommit`), `VoteTargetType` (`block`/`nil`).

This ADR builds the first real content in `hn-consensus`: the round
state machine itself, `ConsensusState` plus its transitions, exactly as
scoped — pure logic, no networking, no real timers, no node process.
The driving caller (a future `hn-node`) supplies already-verified
inputs (a proposal, an already-quorum-checked `QuorumCertificate`, or a
timeout notification it decided to fire) as explicit events; this crate
decides only what state comes next and what the local validator should
do about it (cast a prevote/precommit, finalize, advance a round) — it
never verifies a signature, computes a quorum, or checks validator
eligibility itself, mirroring the same "this layer trusts the caller
already resolved X" boundary `ConsensusVote::verify`/
`QuorumCertificate::verify_signatures` already draw one layer down.

## Decision

**`ConsensusState`**, new in `hn-consensus`:

```rust
pub struct ConsensusState {
    pub height: BlockHeight,
    pub round: Round,
    pub step: ConsensusStep,
    pub locked_block: Option<Digest>,
    pub highest_qc: Option<QuorumCertificate>,
}
```

**`ConsensusStep`**: `NewHeight`, `Propose`, `Prevote`, `Precommit`,
`Finalize`, `Timeout` — six explicit, directly observable/assertable
values, matching the exact chain named for this pass (`NEW_HEIGHT ->
PROPOSE -> PREVOTE -> PRECOMMIT -> FINALIZE/TIMEOUT`). `Finalize`/
`Timeout` are real resting steps here, not just descriptions of an
edge, specifically so a test can assert `state.step ==
ConsensusStep::Finalized` immediately after feeding a qualifying
`precommit` quorum, before deciding what happens next (extracting the
finalized block, moving on to `NewHeight`) — this is what "tested in
isolation" needs: every step the diagram names is a value the state
machine can actually be caught in, not an implicit transient.

**The transition table** (`ConsensusState::apply(&mut self, event:
ConsensusEvent) -> ConsensusResult<ConsensusAction>`):

| Step | Event | New step | `round` | `locked_block`/`highest_qc` | Action |
|---|---|---|---|---|---|
| `NewHeight` | `BeginRound` | `Propose` | unchanged | unchanged (fresh) | none |
| `Propose` | `Proposal{block_hash, justification}` | `Prevote` | unchanged | unchanged | `Prevote(decide_target(...))` — see "Locking rule" |
| `Propose` | `ProposeTimeout` | `Prevote` | unchanged | unchanged | `Prevote(Nil)` |
| `Prevote` | `PrevoteQuorum(qc)`, `qc` targets a block | `Precommit` | unchanged | `locked_block = Some(qc.target_hash)`, `highest_qc = Some(qc)` | `Precommit(Block(qc.target_hash))` |
| `Prevote` | `PrevoteQuorum(qc)`, `qc` targets nil | `Precommit` | unchanged | unchanged | `Precommit(Nil)` |
| `Prevote` | `PrevoteTimeout` | `Precommit` | unchanged | unchanged | `Precommit(Nil)` |
| `Precommit` | `PrecommitQuorum(qc)`, `qc` targets a block | `Finalize` | unchanged | unchanged | `Finalized{block_hash, commit_qc: qc}` |
| `Precommit` | `PrecommitQuorum(qc)`, `qc` targets nil | `Timeout` | `round + 1` | unchanged (persists into the next round — this is the entire reason locking exists) | `RoundAdvanced{round}` |
| `Precommit` | `PrecommitTimeout` | `Timeout` | `round + 1` | unchanged | `RoundAdvanced{round}` |
| `Timeout` | `BeginRound` | `Propose` | unchanged (already incremented) | unchanged | none — the new round number was already reported by `RoundAdvanced` when `Timeout` was entered |
| `Finalize` | `BeginNewHeight` | `NewHeight` | reset to `Round::FIRST` | reset to `None`/`None` | `NewHeight{height}` — the next height number, not reported anywhere earlier |

Every other (step, event) pair is `Err(ConsensusError::UnexpectedEvent)`
— a deliberately closed, total function: this "pure logic, testable in
isolation" scope means every input either has one well-defined outcome
or is rejected, never silently ignored or guessed at. `height`
increments via `BlockHeight::checked_next`, `round` via
`Round::checked_next`, both propagating `ConsensusError::HeightOverflow`/
`RoundOverflow` rather than wrapping — the same checked-arithmetic
discipline every `apply_*` function in `hn-state` already uses.

**Locking rule** (`Propose` step, deciding what to prevote):

```text
decide_target(locked_block, highest_qc, block_hash, justification):
  if locked_block is None: Block(block_hash)
  if locked_block == Some(block_hash): Block(block_hash)
  else:
    // locked on a DIFFERENT block — only unlock given a newer Polka
    if justification is Some(qc)
       and qc targets block_hash
       and qc.round >= highest_qc.round (or highest_qc is None):
      Block(block_hash)
    else:
      Nil
```

This is a deliberate simplification of Tendermint's textbook algorithm,
which tracks two *separate* pairs — `(lockedValue, lockedRound)` (what
this validator has committed to protecting) and `(validValue,
validRound)` (the most recent value it has seen a Polka for, whether or
not it is locked on it) — merged here into the one `highest_qc` field
the user asked for. `highest_qc` plays both roles: it is what
`locked_block` is locked *with* (updated together, only on a
block-targeting `PrevoteQuorum`), and it is the evidence checked when
deciding whether a differently-proposed block may unlock it. Real
Tendermint keeps these independent because a validator can observe a
Polka for a value at a later round than its own lock without
necessarily being the value the lock protects — collapsing them here
means this skeleton is not yet a byzantine-safe implementation of the
full algorithm, only its structural shape. Named explicitly, not
silently glossed over — see "Explicitly Not Resolved."

**Boundary: `apply` trusts its `event` completely.** No signature
verification, no quorum-threshold recomputation, no validator
eligibility check, no `height`/`round`/`epoch`/`chain_id` binding
check against the event's own `QuorumCertificate` fields — all of that
is the caller's job, done before constructing the event, mirroring the
exact "this layer only does state-transition logic; the caller already
resolved state / verified signatures" boundary every `hn-state`
`apply_*` function and `ConsensusVote::verify`/
`QuorumCertificate::verify_signatures` already draw. This crate holds
no signing key and does no networking — matching its own crate-level
doc comment, now finally exercised rather than just asserted.

## Explicitly Not Resolved

**Not byzantine-safe as specified — see "Locking rule," above.**
`highest_qc` conflating `lockedRound`/`validRound` is a real, named
simplification. A future pass implementing this state machine's actual
consensus-safety proof (ADR-0009's own "Test And Verification
Requirements": "deterministic state-machine tests," "formal modeling
... required before any novel consensus variant is accepted") should
revisit whether the merged field is sufficient or whether the two
textbook fields must be tracked separately.

**No height/round scoping check on incoming events.** `apply` assumes
`event`'s own `QuorumCertificate` (when present) already targets this
exact `height`/`round` — it does not check `qc.height == self.height`
or `qc.round == self.round` itself. A future routing/dispatch layer
(part of the eventual `hn-node` driver, not this crate) is the natural
place for that, the same way this crate does not verify signatures
either.

**No leader election integration.** `Propose`'s `Proposal` event takes
a bare `block_hash` — this state machine does not check that the
proposal actually came from the round's correct proposer (ADR-0011).
That check composes `hn_state::active_key`-resolved validator identity
with leader-election output the caller already has; folding it in here
would duplicate a concern this crate's own modular-boundary charter
(ADR-0009, "Modular Boundaries") says belongs to a separate module.

**No `FinalityProof` construction.** `ConsensusAction::Finalized`
exposes exactly `{ block_hash, commit_qc }` — the two ingredients
ADR-0013 already named as sufficient ("one `precommit`
`QuorumCertificate` is sufficient") — but does not wrap them in the
`FinalityProof` conceptual struct ADR-0013 sketches (`proof_version`,
`consensus_profile`, `chain_id`, `network_id`, `epoch`,
`validator_set_commitment`, `finality_metadata`), which has no concrete
Rust type anywhere in this codebase yet. Building that type is real,
separate follow-up work.

**No networking, timers, mempool, or block production.** Exactly as
scoped: this ADR is the round state machine only. Actually driving it
from real network messages and real timers is `hn-node`'s job, not
begun here.

## Rejected Options

### Tracking `locked_round`/`valid_round` As Two Separate Fields, Matching Textbook Tendermint Exactly

Considered, and named as the more textbook-correct choice in "Locking
rule" above. Deferred rather than rejected outright: the user's own
scope for this pass named exactly `locked_block`/`highest_qc`, and a
merged first skeleton that is honest about the simplification is more
useful right now than a half-finished four-field version. Revisiting
this is explicitly left open, not foreclosed.

### `apply` Returning `Vec<ConsensusAction>` Instead Of One

Rejected: every transition in the table above produces exactly one
action. A `Vec` would suggest some transitions might need several,
which none do — matching this project's own "don't design for
hypothetical future requirements" discipline.

### Silently Ignoring An Event That Doesn't Match The Current Step

Rejected: a state machine meant to be "tested in isolation" needs
every input to have a defined, assertable outcome. Silently no-op'ing
an out-of-step event would make a whole class of bugs (a caller
misrouting an event) invisible to tests instead of surfacing as a clear
`Err(ConsensusError::UnexpectedEvent)`.

## Security Considerations

Locking-rule simplification (see "Explicitly Not Resolved"):

- Risk: merging `lockedRound`/`validRound` into one `highest_qc` field
  could, in a scenario textbook Tendermint's two-field design
  distinguishes, allow an incorrect unlock decision.
- Mitigation: none in this pass — named explicitly as unresolved, not
  silently assumed safe. No safety claim is made for this skeleton
  beyond "structurally shaped like Tendermint," per ADR-0009's own
  "Explicit Safety Model" rule that claims require documented
  assumptions and failure modes.

Trusting unverified events:

- Risk: if a future caller feeds `apply` an event built from an
  unverified `QuorumCertificate` (wrong signatures, insufficient real
  quorum, wrong validator set), this state machine will act on it
  without complaint.
- Mitigation: this is a scope boundary, not an oversight — verification
  is explicitly the caller's job (see "Boundary," above), the same
  division already proven out for `ConsensusVote`/`QuorumCertificate`
  themselves.

## Compatibility

Purely additive: new types and functions in a previously-empty crate.
No changes to any existing type's wire encoding or public signature.

## Open Decisions

- `lockedRound`/`validRound` field separation (see "Explicitly Not
  Resolved")
- height/round scoping validation for incoming events
- leader-election integration (proposer eligibility check)
- `FinalityProof` concrete type
- everything ADR-0009 itself still lists open (timeout durations,
  networking channels, formal modeling framework, ...)

## Related Specifications

- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0011-leader-election.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0013-finality-rules.md`
- `docs/rfc/consensus/consensus-architecture.md`
