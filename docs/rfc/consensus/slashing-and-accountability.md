# HNChain Consensus RFC: Slashing And Accountability

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0001-account-state-model.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0013-finality-rules.md`
- `docs/adr/ADR-0014-fork-choice-rules.md`
- `docs/adr/ADR-0015-slashing-and-accountability.md`
- `docs/adr/ADR-0016-synchronization-checkpoints.md`

## 1. Purpose

This RFC defines the conceptual accountability model for HNChain validators.

It specifies evidence structure, evidence verification, penalty boundaries, and
open decisions required before any economic slashing is activated.

## 2. Scope

This RFC defines:

- evidence object requirements
- evidence type categories
- evidence verification inputs
- status-change boundaries
- slashing preconditions
- security requirements
- test vector requirements

This RFC does not define:

- final staking economics
- final slashing amounts
- final delegation model
- final unbonding period
- final downtime penalty policy
- final governance override process

## 3. Evidence Object

Conceptual structure:

```text
ConsensusEvidenceV1
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

All fields that affect validity must be HNCS-encoded.

## 4. Evidence Types

Initial conceptual evidence categories:

```text
double_proposal
double_vote
conflicting_qc_participation
conflicting_finality_participation
invalid_consensus_signature
safety_rule_violation
```

The accepted consensus profile must define which evidence types are active.

## 5. Evidence Verification

Evidence verification requires:

- supported evidence version
- supported evidence type
- matching chain and network
- matching consensus profile
- valid accused validator identity
- valid validator set commitment
- canonical offending objects
- valid signatures on offending objects
- exact conflict rule match
- evidence within validity window, if any

Verification must be deterministic and bounded.

## 6. Penalty Classes

Potential accountability actions:

```text
record_only
jail
temporary_inactive
reward_reduction
slash
exit_forced
```

`slash` is not activated by this RFC.

Every active penalty class must define state transition rules and account
effects.

## 7. Evidence Inclusion

Evidence may be included in blocks through the block evidence section and
committed by `evidence_root`.

Evidence inclusion must define:

- maximum evidence count
- maximum evidence byte size
- evidence ordering
- duplicate handling
- fee behavior
- effect timing
- receipt or event behavior

**Decided** (ADR-0015, "Decided: evidence digest mechanism"):
`evidence_root` formula and evidence ordering (sorted by ascending
`evidence_hash`, not inclusion order — ADR-0015). Maximum count/byte
size, duplicate handling, fee behavior, effect timing, and receipt/event
behavior remain open (§13) — this only closes the commitment-mechanism
level, not every aspect of inclusion.

## 8. Jailing

Jailing changes validator status.

Jailing rules must define:

- evidence types that cause jailing
- jail duration or release condition
- activation epoch
- voting power effect
- reward effect
- key rotation interaction
- reactivation rule

**Decided** (ADR-0015, "Decided: jailing activation mechanism"): all
five defined `evidence_type` values cause jailing, automatically and
deterministically upon evidence acceptance — no operator/governance
step. Takes effect immediately (the height after the evidence-including
block), not at the next epoch boundary: a jailed validator is excluded
from signing and from `total_voting_power` right away, layered as a
live check on top of the epoch-frozen `validators_root` rather than
forcing an early re-snapshot — QC/vote verification needs both epoch
membership *and* a live not-jailed check. Reactivation reuses the
already-decided `validator_activate` operation from `inactive` (no
separate reactivation rule); key rotation interaction is none beyond
what `validator-set.md` §10 (Key Rotation) already specifies. Jail
duration itself and reward effect (blocked entirely — no reward
mechanism exists yet) remain open (§13).

## 9. Slashing Preconditions

Before slashing can activate, specifications must define:

- bonded stake source
- delegation model
- delegator loss model
- unbonding period
- penalty amounts
- correlated failure policy
- evidence window
- governance constraints
- incident response procedure
- accounting and receipt format

Without these, slashing remains disabled.

## 10. Downtime Accountability

Downtime is different from Byzantine equivocation.

Downtime policy must define:

- measurement source
- missed vote window
- expected network assumptions
- validator self-reporting irrelevance
- false-positive controls
- penalty class

Downtime slashing should not be activated without extensive testnet evidence.

**Out of scope for the jailing decision** (ADR-0015, "Decided: jailing
activation mechanism"): jailing's trigger is Byzantine-equivocation
evidence only. This is not a narrower restatement of the sentence
above — ADR-0015's own "Penalizing Based On Downtime Alone Without
Rules" (Rejected Options) already rejects triggering *any*
accountability action, jailing included, from downtime alone until
this section's own policy items are defined.

## 11. Security Requirements

Implementations must reject:

- unknown evidence versions
- unknown active evidence types
- evidence for another chain or network
- evidence for unsupported consensus profiles
- malformed offending objects
- invalid signatures
- unsupported validator set commitments
- evidence outside validity window
- duplicate evidence, if already applied
- evidence with ambiguous conflict rules
- oversized evidence payloads

Implementations must bound:

- evidence decode cost
- signature verification count
- validator set proof size
- evidence cache size
- evidence inclusion per block
- evidence verification time

## 12. Test Vectors

The accepted version must include test vectors for:

- valid double-vote evidence
- valid double-proposal evidence
- non-conflicting votes rejection
- wrong chain rejection
- wrong epoch rejection
- invalid signature rejection
- duplicate evidence handling
- evidence outside validity window
- jailing state transition
- slashing disabled behavior
- malformed evidence rejection

Test vectors are mandatory before production implementation.

## 13. Open Decisions

- final evidence type registry
- final evidence schemas
- final evidence validity window
Economic-value items below are owned by ADR-0023 (Tokenomics And
Economic Model), not this document.

- final evidence inclusion limits and behavior (root formula and
  ordering decided; count/size limits, duplicate handling, fee
  behavior, effect timing, and receipt/event behavior remain open; see
  §7)
- final jail duration / release condition (activation mechanism
  decided; see §8 — ADR-0023)
- final downtime policy (deliberately excluded from jailing's scope;
  see §10)
- final slashing activation criteria (ADR-0023)
- final slashing economics (ADR-0023)
- final delegator impact model (ADR-0023)
- final unbonding interaction (ADR-0023)
- final correlated failure policy (ADR-0023)
- final evidence fee policy (ADR-0023)
- final test vector suite
