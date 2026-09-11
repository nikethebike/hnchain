# ADR-0015: Slashing And Accountability

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0013: Finality Rules
- ADR-0014: Fork-Choice Rules

Supersedes: None

## Context

Accountability defines how HNChain detects and proves validator misbehavior.

Slashing is an economic penalty that may be applied after misbehavior is proven.
It is not the same thing as evidence, jailing, missed rewards, or validator
status changes.

HNChain must define canonical evidence before activating any punitive economic
mechanism. A validator must never be penalized based on local logs, operator
claims, RPC responses, or ambiguous message interpretation.

## Decision

HNChain defines a versioned accountability model with canonical evidence
objects.

Conceptual evidence:

```text
ConsensusEvidence
  evidence_version
  evidence_type
  chain_id
  network_id
  consensus_profile
  epoch
  height
  round
  accused_validator
  validator_set_commitment
  evidence_payload
  evidence_hash
```

Initial evidence categories:

- double proposal
- double vote
- conflicting quorum certificate participation
- conflicting finality proof participation
- invalid consensus signature
- safety-rule violation, if profile-defined

This ADR does not activate economic slashing amounts.

The initial accountability action may include evidence recording, validator
jailing, temporary inactivity, reward reduction, or future slashing after
staking economics are specified.

## Normative Rules

### Evidence Before Penalty

Any penalty that affects validator status, rewards, stake, or account balances
must be based on canonical evidence accepted by consensus rules.

### Versioned Evidence

Every evidence object includes `evidence_version`.

Nodes must not infer evidence format from payload size, vote type, signature
algorithm, network message type, or client software.

**Decided: `evidence_type` registry**, `u8`, closed for this profile:

```text
0x00  reserved, invalid
0x01  double_proposal
0x02  double_vote
0x03  conflicting_qc_participation
0x04  conflicting_finality_proof_participation
0x05  invalid_consensus_signature
```

`safety-rule violation, if profile-defined` (Decision, above) is
deliberately left unassigned: it is explicitly conditional on a future
consensus profile defining its own additional safety rule, which does
not exist yet — assigning it a number now would be guessing at
something with no content behind it, the same reasoning already
applied to leaving `hnchain.block.id.v1` (ADR-0005) unassigned.

**Decided: evidence digest mechanism**, closing "evidence inclusion
format" (Open Decisions, below) at the commitment-mechanism level:

```text
evidence_hash = HASH_PROFILE_0x0001(
  "hnchain.evidence.v1", HNCS(ConsensusEvidence))

evidence_root = MTH(included_evidence, by ascending evidence_hash)
```

`MTH` is `hn-list-merkle-v1` (ADR-0008, "Ordered List Commitment"),
the same reuse already applied to `validators_root` (ADR-0010). Sorted
by the evidence objects' own digests rather than "inclusion order,"
avoiding the need for a separate proposer-ordering rule purely for
evidence — unlike `transactions_root`, where order is itself
consensus-relevant (ADR-0008, "Transactions Root"), nothing in this
protocol currently assigns evidence order any meaning.

### Evidence Context Binding

Evidence must bind to:

- chain ID
- network ID
- consensus profile
- evidence type
- epoch
- height
- round, if applicable
- validator set commitment
- accused validator identity
- canonical offending objects

Evidence valid on one chain or network must not be replayable on another.

### Canonical Offending Objects

Evidence payloads must contain canonical consensus objects or canonical hashes
with enough proof material to verify the claim.

Allowed conceptual inputs include:

- signed proposals
- signed votes
- quorum certificates
- finality proofs
- validator set proofs
- profile-defined safety-state proofs, if any

Local logs and peer reports are not evidence.

### Double Vote

Double-vote evidence proves that the same validator signed two conflicting votes
in the same safety domain.

The consensus profile must define conflict domains precisely.

### Double Proposal

Double-proposal evidence proves that a proposer signed conflicting proposals for
the same height, round, epoch, and proposal domain.

### Conflicting Finality Participation

If a validator signs messages that contribute to conflicting finalized blocks,
the evidence format must prove participation in both conflicting certificates or
finality proofs.

### Penalty Determinism

Penalty outcomes must be deterministic.

If a penalty is activated, the rule must define:

- evidence validity window
- penalty type
- penalty amount or status change
- repeat-offense behavior
- interaction with unbonding
- appeal or correction mechanism, if any
- effect on delegators, if delegation exists

### Jailing

Jailing is a validator status change that removes or prevents active consensus
participation for a defined period or condition.

Jailing may be activated before monetary slashing if the evidence rules are
accepted and validator lifecycle rules support it.

**Decided: jailing activation mechanism.** The evidence rules referenced
above are already accepted (`evidence_type` registry, evidence digest
mechanism, Evidence Context Binding — all decided earlier in this ADR),
and validator lifecycle already supports it (`active → jailed →
inactive`, ADR-0010's "Decision," penalty path) — so, per this
section's own precondition, jailing is activated now.

- **Trigger.** Any of the five defined `evidence_type` values (`double_
  proposal`, `double_vote`, `conflicting_qc_participation`,
  `conflicting_finality_proof_participation`, `invalid_consensus_
  signature`) causes jailing automatically and deterministically upon
  the evidence being accepted by consensus rules — no separate human,
  governance, or operator step. Not an independent choice: "Evidence
  Before Penalty" (above) already requires this, and "Manual Operator
  Slashing" (Rejected Options) already rejects any discretionary
  alternative. `safety_rule_violation` causes jailing too, once and if
  a future consensus profile ever defines it — it is not a distinct
  case requiring its own rule.
- **Timing — immediate, via a live overlay.** Asked the user explicitly
  — genuinely two viable designs, comparable in weight to the capping
  algorithm decision (ADR-0010), not a derived call. Decided: jailing
  takes effect starting at the height *after* the evidence-including
  block, not at the next epoch boundary. A jailed validator is excluded
  from signer eligibility and from `total_voting_power` immediately;
  `validators_root`/`consensus_root` (ADR-0010) stay epoch-frozen as
  already decided — jailing does not force an early re-snapshot. This
  means QC/vote verification needs a live status check *in addition to*
  epoch-committed validator set membership: `validator_id` must be (a) a
  member of the epoch's committed `validators_root` **and** (b) not
  currently `jailed` as of the height being verified. This is a real,
  deliberate addition to what the still-missing active-set query
  interface (`QuorumCertificate::decode`'s own documentation already
  flags this gap) must eventually expose — not a detail this decision
  can leave implicit. Chosen over the simpler epoch-delayed alternative
  because the alternative would let a validator caught equivocating
  keep signing and contributing to `total_voting_power` for up to a
  full epoch after being caught, undermining jailing's actual
  containment purpose during exactly the window it matters most.
- **Reactivation.** No special-case rule: jailing's own lifecycle edge
  already lands on `inactive` (ADR-0010's penalty path,
  `active → jailed → inactive`), and `inactive → active` already has a
  mechanism — the same explicit, deliberate `validator_activate`
  operation ADR-0010's "Decided: admission mechanism" already defined
  for any not-currently-active validator meeting the minimum bond. A
  formerly-jailed validator does not silently resume consensus duty the
  moment a jail condition lapses, for the same reason admission itself
  is opt-in rather than automatic: the operator should choose when
  they're ready to accept that exposure again, not have it imposed by a
  timer alone.
- **Key rotation interaction.** None beyond what ADR-0010's own "Key
  Rotation" already specifies — jailing does not add or remove any key
  rotation rule; a jailed validator's keys are neither frozen nor forced
  to rotate by this decision.
- **What stays open.** The jail *duration* or release condition itself
  (a tunable constant — same later-batch nature as `unbonding period`,
  not decided here) and `reward effect` (blocked entirely: no reward
  mechanism exists anywhere in this project yet, not just unspecified
  amounts).

**Downtime is explicitly out of scope for this decision.** Not a fresh
open question so much as already resolved by existing text: "Penalizing
Based On Downtime Alone Without Rules" (Rejected Options, below) already
rejects triggering *any* accountability action — jailing included, not
only monetary slashing — from downtime alone, "until... clear measurement
windows and fault assumptions" exist, and `slashing-and-accountability.md`
§10 already says the same ("should not be activated without extensive
testnet evidence"). This decision's jailing trigger is Byzantine-
equivocation evidence only; it does not extend jailing to downtime.

### Slashing

Slashing is not activated by this ADR.

Before slashing can be activated, HNChain must specify:

- staking and bonding model
- delegation model, if any
- unbonding period
- penalty amounts
- reward accounting
- delegator impact
- evidence windows
- governance limits
- recovery and incident response

**Deliberately untouched by this decision pass** — jailing, above,
does not imply or bring slashing closer to activation; every item in
this list is owned by ADR-0023 (Tokenomics And Economic Model), the
same document that now owns voting power's exact
`cap_numerator`/`cap_denominator` and minimum validator bond (ADR-0010)
— one owning ADR, not a staking/delegation/tokenomics track scattered
across several.

## Rejected Options

### Manual Operator Slashing

Rejected because punishment must not depend on human discretion or private
infrastructure.

### Slashing Without Canonical Evidence

Rejected because ambiguous punishment is a protocol safety and governance risk.

### Penalizing Based On Downtime Alone Without Rules

Rejected because network failures, partitions, and client bugs must be handled
with explicitly specified liveness and performance rules.

Downtime penalties may be considered later with clear measurement windows and
fault assumptions.

### Immediate Balance Confiscation In This ADR

Rejected because staking economics are not accepted yet.

### Evidence From RPC Responses

Rejected because RPC responses are not consensus objects.

## Alternatives Considered

### Evidence Recording Only

Advantages:

- safest early accountability mechanism
- supports public audit
- avoids premature economic penalties

Disadvantages:

- weak immediate deterrence
- requires social or governance response until penalties activate

### Jailing Without Slashing

Advantages:

- removes faulty validators from active participation
- avoids immediate economic confiscation
- useful before full tokenomics are finalized

Disadvantages:

- weaker deterrence than slashing
- may be abused if evidence rules are too broad

### Fixed Slashing Amounts

Advantages:

- simple to understand
- strong deterrence for clear equivocation

Disadvantages:

- premature without economic modeling
- may over-penalize during client bugs or correlated failures
- delegator impact is complex

### Proportional Slashing

Advantages:

- can scale penalty with severity or stake
- may improve incentive alignment

Disadvantages:

- more complex economics
- harder to explain and audit
- correlated slashing risk must be modeled carefully

## Security Considerations

False evidence:

- Risk: attackers submit malformed or misleading evidence.
- Mitigation: canonical evidence formats, strict verification, and bounded
  processing.

Ambiguous conflict rules:

- Risk: honest validators are penalized due to unclear safety domains.
- Mitigation: consensus profile must define exact conflict conditions.

Key compromise:

- Risk: stolen keys cause slashable signatures.
- Mitigation: key separation, hardware signing, rotation delay, and operational
  guidance.

Mass slashing:

- Risk: client bug or ambiguous upgrade causes many validators to be penalized.
- Mitigation: staged activation, testnets, conservative evidence rules, and
  incident response policy.

Evidence spam:

- Risk: attackers flood nodes with expensive evidence.
- Mitigation: size limits, cheap prechecks, evidence fees, and bounded
  verification.

Governance capture:

- Risk: governance changes slashing rules to punish opponents.
- Mitigation: constitutional limits, delayed activation, and compatibility
  review.

Delegator harm:

- Risk: delegators lose funds for operator mistakes.
- Mitigation: delegation risk disclosure and explicit delegator-impact rules
  before slashing activation.

## Compatibility

Adding a new evidence type is a consensus change unless it is explicitly
non-punitive and ignored by old nodes.

Activating monetary slashing is a major protocol change.

Changing penalty amounts, evidence windows, or validator status effects requires
activation rules and compatibility analysis.

Historical evidence verification must remain possible after cryptographic
algorithm migrations.

## Open Decisions

Economic-value items below (evidence fees, jail duration constant,
slashing activation/amounts, delegator impact, unbonding interaction,
correlated failure policy) are owned by ADR-0023 (Tokenomics And
Economic Model), not this ADR.

- evidence validity window
- evidence fees (ADR-0023)
- jail duration / release condition (trigger, timing, reactivation, and
  key rotation interaction all decided above — "Decided: jailing
  activation mechanism"; only the duration constant remains — ADR-0023)
- slashing activation criteria (ADR-0023)
- slashing amounts (ADR-0023)
- delegator impact (ADR-0023)
- unbonding interaction (ADR-0023)
- correlated failure policy (ADR-0023)
- incident response path
- evidence proof size limits
- evidence test vector suite

## Related Specifications

- `docs/rfc/consensus/slashing-and-accountability.md`
- `docs/rfc/consensus/synchronization-checkpoints.md`
