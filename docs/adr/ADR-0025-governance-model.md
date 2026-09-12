# ADR-0025: Governance Model

Status: Proposed

Date: 2026-09-12

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0003: Address Format
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0010: Validator Set Model
- ADR-0023: Tokenomics And Economic Model

Supersedes: None

## Context

`governance` (`tx_type = 0x07`) has been reserved in ADR-0006's closed
registry since that ADR's own tx_type decision, parked with the same
blocker text as every other undecided type: "blocked on a governance
model, which does not exist yet." ADR-0023's own economic-parameter
batch pass closed part of that gap — "Decided: Governance Voting
Model": validators and stakers vote as two separate chambers, both
required to agree, explicitly rejecting `1 HNC = 1 vote` as a default
(whitepaper §12.7) — but left "proposal process" and "scope of what
governance may decide" as its own named Open Decisions. A payload
cannot be written from a voting-weight model alone: it needs something
concrete to vote *on*. This ADR is that proposal model.

ADR-0007 separately mapped the `governance` state domain (`0x0007`) as
a pure singleton, decided before any governance model existed to test
that cardinality claim against. This ADR's own decision (individual
proposals, each needing their own record) already forced a correction
there — governance now holds a singleton-plus-collection shape, the
same pattern `bridge` (`0x000A`) already established — recorded in
ADR-0007 directly, not restated here.

No execution engine exists anywhere in this project (HNVM has no
design, `contract_deploy`/`contract_call` are still fully blocked on
it) capable of carrying out an arbitrary on-chain effect a passed
proposal might demand. This ADR does not invent one to make governance
"do" something — see "Decided: Signaling Only," below.

## Decision

### Decided: Signaling Only

A passed proposal has no automatic on-chain effect. Voting is real and
binding as a recorded signal — a passed proposal is a canonical,
verifiable statement that both chambers agreed — but nothing in this
protocol reads that outcome and changes behavior because of it. Any
actual protocol change a proposal calls for (a parameter value, an
activation, an upgrade) still requires its own explicit process: a new
or amended ADR, implementation, and activation, exactly like every
other protocol change in this project. This mirrors the same
now-established pattern jailing already used ahead of slashing —
activate a mechanism now, without inventing teeth for it before its
preconditions (here, a general execution engine) exist.

An "urgent curated action registry" alternative (a small, hand-picked
set of on-chain effects governance could trigger without full HNVM,
such as toggling slashing activation) was considered and rejected for
this pass — see Rejected Options.

### Decided: Chambers, Membership, And Weight

Two chambers, both required to pass independently for a proposal to
pass overall (ADR-0023's own "validator + staker chambers" decision,
made concrete here):

- **Validator chamber.** Membership: every validator with
  `status = Active` at the proposal's creation height. Weight: **one
  validator, one vote** — not `voting_power`-weighted. This is the
  entire reason a second chamber exists rather than one stake-weighted
  pool: the staker chamber already gives capital its say; the
  validator chamber gives every active operator an equal one,
  protecting smaller validators from being outvoted purely by stake
  concentration within their own chamber.
- **Staker chamber.** Weight: stake-weighted, by bonded stake. A
  validator is also a staker of their own bonded funds and so has a
  voice in both chambers simultaneously, weighed on two different
  dimensions (operator standing vs. capital) — not a double-count,
  since nothing here sums the two chambers' tallies together.

**Scoping note, stated plainly rather than glossed over**: delegation
is decided as supported (ADR-0023) but its own transaction/state
mechanism is not yet designed — `stake`/`unstake` are sender-only
today (ADR-0006), so there is currently no per-delegator record
distinguishing a delegator's own stake from a validator's self-bonded
stake. Until delegation's own mechanism exists, the staker chamber's
weight is computed from what is actually trackable today: each
validator's own `ValidatorRecordV1.bonded_stake`. This is not a design
flaw specific to governance — it is the same limitation ADR-0023's own
"Decided: Delegation Supported" already named. Once delegation gets a
real per-delegator mechanism, the staker chamber's weight source
extends to include it without needing to revisit the chamber concept
itself.

Both chambers' membership and total weight are snapshotted at the
proposal's creation height and held fixed for that proposal's entire
voting window — the same epoch-snapshot discipline ADR-0010 already
established for the active set, applied here to avoid a moving quorum
target as validators join, leave, or restake mid-vote.

### Decided: Chamber Pass Rule

Per chamber: a **quorum** of the chamber's total weight must
participate (vote `for`, `against`, or `abstain` — not merely exist as
a member) before the vote counts as decided at all; among participating
weight, a **simple majority** of non-abstaining weight must vote `for`.
`abstain` counts toward quorum but not toward the for/against ratio —
a participant that intentionally declines to take a side still
signals engagement, distinct from not voting at all.

The quorum *percentage* is deliberately not decided here — a tunable
economic parameter, the same class of decision as the voting-power cap
ratio or minimum bond (ADR-0023), not a structural one. See Open
Decisions.

### Decided: Proposal Outcome States

```text
Voting -> Passed
       -> Rejected
       -> Expired
```

At the voting window's close (Decided: Voting Window, below):

- **Expired**: quorum was not met in at least one chamber. Not enough
  of the network engaged with the proposal to produce a meaningful
  signal either way — distinct from an active rejection.
- **Rejected**: quorum was met in both chambers, but the `for`/`against`
  majority failed in at least one of them.
- **Passed**: quorum was met and a `for` majority was reached in
  *both* chambers independently.

### Decided: Voting Window

Height-based, fixed duration — the same choice already made for
`ValidityWindowV1` (ADR-0006) and epoch boundaries (ADR-0010) over a
wall-clock alternative, for the same reason: `BlockHeader.timestamp`'s
own consensus semantics remain undecided (ADR-0008). The exact
duration (`GOVERNANCE_VOTING_WINDOW`, in blocks) is not decided here —
a tunable constant owned by ADR-0023, the same mechanism-now/amount-
later split already applied to `EPOCH_LENGTH` and the unbonding
period.

### Decided: Who May Propose

Any validator with `status = Active` at proposal-creation time.
Requiring active bonded participation is itself the anti-spam barrier
— no separate deposit mechanism is introduced. This reading of "bonded
validator" excludes `Candidate` (not yet admitted to consensus) and
`Inactive`/`Jailed` (not currently participating) — stated explicitly
since "bonded" alone does not, by itself, disambiguate among every
non-`Exited` status.

### Decided: Proposal Content

```text
GOVERNANCE_PROPOSAL_TITLE_MAX_LEN = 256 bytes
```

A proposal carries a short, bounded UTF-8 `title` on-chain (bounded the
same way the genesis message is, `docs/specs/core/genesis.md` §4 —
neutral, durable identifying text, not a place to inline an entire
proposal's argument) plus a `content_hash: Digest` — a document
commitment (the same pattern genesis's own "Document Commitments,"
§6, already uses) to the full proposal text, which lives off-chain.
Consensus verifies and commits to the hash, never the prose; where the
full text is published, and in what format, is not a consensus concern
(the same boundary genesis's own document-commitment process already
draws).

### Decided: Transaction Payload Shape

`governance` (`tx_type = 0x07`) carries a discriminated payload,
mirroring `ValidatorUpdatePayloadV1`'s own already-established
`operation`-plus-conditional-fields pattern (ADR-0006) rather than a
boxed-variant encoding:

```text
GovernancePayloadV1
  u16   payload_version = 1
  u8    operation
  -- Propose --
  bytes title      (bounded, <= 256 bytes, present iff operation = Propose)
  bytes32 content_hash                    (present iff operation = Propose)
  -- Vote --
  bytes32 proposal_id                     (present iff operation = Vote)
  u8    choice                            (present iff operation = Vote)
```

```text
GovernanceOperation (u8, closed for this payload version)
  0x00  reserved, invalid
  0x01  propose
  0x02  vote
```

```text
VoteChoice (u8, closed for this payload version)
  0x00  reserved, invalid
  0x01  for
  0x02  against
  0x03  abstain
```

No `validator_id`/chamber field on `Vote`: `sender` (ADR-0006) already
identifies the voter; which chamber(s) they vote in, and with what
weight, is derived from canonical state at the proposal's snapshot
height (Decided: Chambers, above), the same authorization-is-implicit-
in-sender pattern `stake`/`unstake` already use for `validator_id`.

### Decided: `proposal_id` Derivation

```text
proposal_id = HASH_PROFILE_0x0001(
  "hnchain.governance.proposal.v1", HNCS(GovernancePayloadV1 { Propose fields }))
```

Mirrors `tx_id`'s own derivation exactly (ADR-0006) — a domain-
separated hash over the proposal's own canonical creation content, not
an incrementing counter (which would need a canonical ordering source
this protocol does not otherwise need) or a raw digest of the whole
transaction envelope (which would make `proposal_id` depend on
signature bytes, unlike `Vote`'s own `proposal_id` reference, which
must be stable regardless of anything about the `propose` transaction's
own authorization). A new ADR-0005 domain tag,
`hnchain.governance.proposal.v1`, is added for this.

### Decided: State Shape

Two sections in the `governance` domain (`0x0007`, corrected to a
singleton-plus-collection shape by this ADR — see ADR-0007):

```text
ProposalRecordV1                         (object_id = proposal_id)
  proposal_version
  proposal_id
  proposer                 (bytes32, sender's address_body)
  title
  content_hash
  created_at_height
  voting_ends_at_height
  status                   (Voting | Passed | Rejected | Expired)
  validator_chamber_for / against / abstain   (u128 counts, 1-per-validator)
  staker_chamber_for / against / abstain      (u128 weight sums)
  validator_chamber_total_weight              (snapshotted)
  staker_chamber_total_weight                 (snapshotted)
```

```text
ProposalVoteRecordV1     (object_id = proposal_id, subkey = voter's address_body)
  vote_version
  choice
```

`ProposalVoteRecordV1` exists specifically to reject a second `Vote`
from the same sender on the same proposal (`state_key_core`'s
`object_id` + `subkey` framing already supports exactly this per-
proposal-per-voter keying, ADR-0007) — without it, nothing would
prevent double-voting by resubmitting. `proposal_id` is stored inside
`ProposalRecordV1` itself despite also being that record's own key, the
same redundancy ADR-0010's `ValidatorRecordV1.validator_id` already
accepts and explains: a reader cannot invert a one-way state-key hash
back to the id it committed to.

Exact field widths, encode/decode, and the actual `apply_propose`/
`apply_vote` state transitions are implementation work for a following
pass, mirroring how `stake`/`unstake`/`validator_update`'s own payload
shapes (ADR-0006) were decided before their `apply_*` functions were
written (`hn_state::validator_transition`) — not decided by this ADR.

## Normative Rules

### Signaling Has No Implicit Authority

No component of this protocol may treat a `Passed` proposal as
authorization to change consensus-critical behavior. Doing so would
silently reintroduce exactly the kind of undesigned execution path
"Decided: Signaling Only" exists to avoid.

### Snapshot Determinism

A proposal's chamber membership and total weight are fixed at creation
height and must not be recomputed at any other height for that
proposal's own lifetime, even if the canonical validator/stake state
changes before the voting window closes.

### One Vote Per Sender Per Proposal

A `Vote` from a sender who already has a `ProposalVoteRecordV1` for
that `proposal_id` is invalid — not a silent overwrite, not a
vote-changing mechanism. Vote-changing is not decided by this ADR (see
Open Decisions).

## Rejected Options

### Urgent Curated Action Registry

Rejected for this pass because it commits to a specific list of
governance-triggerable protocol effects (for example, activating
slashing) ahead of deciding governance's own scope generally — itself
still an ADR-0023 Open Decision ("scope of what governance may
decide"). Revisiting this once that scope is decided remains open, not
foreclosed.

### Stake-Weighted Validator Chamber

Rejected because it would make the validator chamber a strict subset
of the staker chamber's own weighting logic, defeating the reason a
second chamber exists at all (Decided: Chambers, above).

### Wall-Clock Voting Window

Rejected for the same reason `ValidityWindowV1` (ADR-0006) already
rejected it: `BlockHeader.timestamp`'s consensus semantics remain
undecided (ADR-0008).

### Vote Tallies Embedded Without Per-Voter Records

Rejected because nothing would then prevent the same sender from
voting on a proposal multiple times, inflating their own effective
weight.

## Alternatives Considered

### Single Stake-Weighted Pool (`1 HNC = 1 vote`)

Advantages:

- simplest possible model
- no chamber-membership bookkeeping

Disadvantages:

- already rejected as a default by the whitepaper (§12.7) and by
  ADR-0023's own governance decision — wealth concentration buys
  direct protocol control

### Delegated Voting

Advantages:

- lets passive holders participate without evaluating every proposal
  themselves

Disadvantages:

- needs its own delegation infrastructure, distinct from staking
  delegation (ADR-0023 already flags this exact distinction)
- not chosen for the initial voting-weight model (ADR-0023)

### Technical Council

Advantages:

- fast, expert-informed decisions

Disadvantages:

- centralizes governance in a small, appointed group — not chosen for
  the initial voting-weight model (ADR-0023)

## Security Considerations

Proposal spam:

- Risk: a validator floods the `governance` tx_type with proposals.
- Mitigation: only `Active` validators may propose — already bonded,
  already subject to the admission/jailing mechanisms ADR-0010/
  ADR-0015 provide; a spamming validator's own consensus standing is
  already at stake by other means.

Low-turnout capture:

- Risk: a small, coordinated group passes a proposal while most
  eligible weight abstains from participating at all.
- Mitigation: the quorum requirement (Decided: Chamber Pass Rule) — a
  proposal with too little participation expires rather than passing,
  regardless of how lopsided the votes that were cast happen to be.

Staker-chamber weight currently narrower than "staker" implies:

- Risk: since delegation's own tracking mechanism does not exist yet,
  the staker chamber's actual weight source (validators' own
  `bonded_stake`) is narrower than what "every staker, including
  delegators" would suggest — a reader could mistakenly assume
  delegators already have direct governance weight today.
- Mitigation: stated explicitly in "Decided: Chambers," above, not
  left implicit; revisit once delegation's own mechanism exists.

False signaling authority:

- Risk: a client or dashboard displays a `Passed` proposal as if it
  were already protocol truth.
- Mitigation: "Signaling Has No Implicit Authority" (Normative Rules)
  — ecosystem tooling must not represent a passed proposal as an
  activated change.

## Compatibility

Activating any on-chain effect for a passed proposal (moving beyond
"Decided: Signaling Only") is itself a future protocol change requiring
its own ADR and activation process — this ADR does not pre-authorize
one. Changing chamber membership rules, weight sources, or the pass
rule after genesis requires the same compatibility review any other
consensus-relevant rule change does.

## Open Decisions

- quorum percentage per chamber (ADR-0023 — mechanism decided above,
  the value is not)
- `GOVERNANCE_VOTING_WINDOW` (ADR-0023 — mechanism decided above,
  height-based fixed duration, the value is not)
- vote-changing (may a sender's `Vote` for a given proposal be
  resubmitted to change their own recorded choice before the window
  closes, or is the first vote final)
- scope of what governance may actually decide, and any future
  execution path once one is designed (deliberately out of scope,
  "Decided: Signaling Only")
- delegation's own per-delegator tracking mechanism (ADR-0023,
  "Decided: Delegation Supported" — blocks the staker chamber's weight
  source from actually including delegators, not just governance's own
  concern)
- proposal cancellation/withdrawal by its own proposer, if any
- `GovernancePayloadV1`/`ProposalRecordV1`/`ProposalVoteRecordV1` exact
  field widths and encode/decode (implementation work, not decided
  here — see "Decided: State Shape")
- `apply_propose`/`apply_vote` state transitions (implementation work,
  following `hn_state::validator_transition`'s own precedent)
- governance minimum proposal spacing or rate limiting, if needed
  beyond "only Active validators may propose"

## Related Specifications

- `docs/whitepaper/HNChain-Whitepaper-v0.1-draft.md` (Chapter XIII,
  §12.7 "Governance")
- `docs/adr/ADR-0023-tokenomics-and-economic-model.md`
- `docs/adr/ADR-0007-state-tree.md` (`governance` domain shape
  correction)
- `docs/specs/core/genesis.md` (document commitment pattern reused for
  `content_hash`)
