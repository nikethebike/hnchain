# ADR-0015: Slashing And Accountability

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
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

- initial evidence type registry
- evidence inclusion format
- evidence validity window
- evidence fees
- evidence root construction
- jailing activation rules
- downtime accountability
- slashing activation criteria
- slashing amounts
- delegator impact
- unbonding interaction
- correlated failure policy
- incident response path
- evidence proof size limits
- evidence test vector suite

## Related Specifications

- `docs/rfc/consensus/slashing-and-accountability.md`
- `docs/rfc/consensus/synchronization-checkpoints.md`
