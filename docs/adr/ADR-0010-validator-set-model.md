# ADR-0010: Validator Set Model

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture

Supersedes: None

## Context

The validator set defines which participants may propose, vote on, and finalize
blocks for a consensus period.

HNChain should support broad validator participation, but validator count cannot
be chosen as a slogan. The active set size, voting power model, hardware
requirements, signature aggregation, networking latency, and finality target are
coupled design parameters.

The validator set model must be explicit before leader election, quorum
certificates, slashing, staking economics, and light-client finality proofs can
be finalized.

## Decision

HNChain defines a versioned validator set model with separate concepts for:

- validator account
- candidate validator
- active validator
- inactive validator
- jailed validator
- exited validator
- validator set commitment

Conceptual lifecycle:

```text
Registered
  -> Candidate
  -> Active
  -> Inactive
  -> Jailed
  -> Exited
```

The validator set used for consensus at a height is derived from canonical state
and committed through block consensus metadata.

The initial voting power model remains open. Candidate models include equal
weight per active validator, stake-weighted voting power, capped stake-weighted
voting power, and committee-based voting power.

## Normative Rules

### Versioned Validator Records

Every validator record includes a schema version.

Nodes must not infer validator semantics from account type, address prefix,
client software, public key length, or RPC metadata.

### Validator Identity

Validator identity is a protocol identity bound to a validator account.

Required conceptual fields:

```text
ValidatorRecord
  record_version
  validator_id
  account_address
  consensus_key
  network_key
  status
  voting_power
  activation_epoch
  deactivation_epoch
  metadata_hash
```

`consensus_key` signs consensus messages.

`network_key` authenticates peer-to-peer node communication when required.

Operational keys must be rotatable without changing account ownership.

### Validator Status

Validator status is consensus state.

Initial conceptual statuses:

- `registered`
- `candidate`
- `active`
- `inactive`
- `jailed`
- `exited`

Status transitions must be deterministic and authorized by account permissions
or protocol rules.

### Active Set Derivation

The active validator set for an epoch or height must be derived deterministically
from canonical state.

Derivation must define:

- eligible statuses
- minimum stake or bond rule, if any
- ranking rule, if active set size is bounded
- tie-breaking rule
- activation delay
- deactivation delay
- maximum active set size, if any
- voting power calculation

### Voting Power

Voting power is a consensus value.

Every accepted consensus profile must define:

- voting power source
- integer type and bounds
- zero-power behavior
- rounding behavior
- overflow behavior
- total power calculation
- quorum threshold calculation

Floating-point arithmetic is rejected for voting power.

### Epoch Boundaries

Validator set changes should occur at deterministic boundaries.

Epoch-based activation is the preferred direction because it makes light-client
verification, checkpointing, and consensus safety easier to reason about.

The exact epoch length is open.

### Validator Set Commitment

Blocks must commit to validator set or validator set transition data through
consensus metadata.

The commitment must be sufficient for:

- full node verification
- light-client finality verification
- checkpoint verification
- historical audit

### Key Rotation

Consensus key rotation must be explicit and delayed.

Immediate key replacement is rejected because it can create ambiguity in
in-flight consensus messages.

Key rotation must define:

- authorization
- activation epoch
- old key validity window
- evidence implications
- light-client verification behavior

### Slashing And Penalties

This ADR does not activate slashing.

If slashing is activated later, penalty rules must be deterministic and based on
canonical evidence.

Validator lifecycle must support non-slashing penalties such as jailing,
temporary inactivity, and reward reduction.

### Metadata

Validator metadata is not consensus authority.

Human-readable metadata may include operator name, website, region, or policy
documents, but consensus must commit only to bounded metadata hashes or
versioned metadata records.

## Rejected Options

### Fixed Validator Count Without Analysis

Rejected because active set size affects decentralization, finality latency,
bandwidth, quorum verification cost, and attack surface.

### One Coin Equals One Vote As Default

Rejected because direct stake weight without caps or delegation design can
increase governance and consensus concentration risk.

Stake-weighted voting remains a candidate, but it requires centralization
analysis and economic modeling.

### Validator Identity As IP Address

Rejected because IP addresses change, can be shared, and are not stable
cryptographic identities.

### Immediate Validator Set Changes

Rejected because changing voting power in the middle of consensus can create
safety and light-client ambiguity.

### Floating-Point Voting Power

Rejected because consensus arithmetic must be deterministic across
implementations and platforms.

## Alternatives Considered

### Equal Weight Active Validators

Advantages:

- simple quorum calculation
- reduces direct stake concentration inside consensus
- easy to test and explain

Disadvantages:

- needs a Sybil-resistance mechanism outside voting weight
- may underprice high-economic-stake validators
- active set admission becomes politically and economically sensitive

### Stake-Weighted Validators

Advantages:

- aligns voting influence with bonded economic exposure
- widely used and familiar in proof-of-stake systems
- simpler incentive mapping

Disadvantages:

- concentration risk
- delegation markets can centralize
- large holders may dominate consensus

### Capped Stake Weight

Advantages:

- preserves some economic weighting
- limits maximum influence of one validator
- can improve decentralization under concentrated stake

Disadvantages:

- may encourage stake splitting
- requires Sybil-resistance and delegation rules
- more complex economics

### Committee-Based Active Set

Advantages:

- can reduce per-block voting overhead
- may support large validator populations
- useful for scaling finality

Disadvantages:

- committee selection randomness becomes critical
- more complex light-client proofs
- additional liveness and censorship risks

## Security Considerations

Stake centralization:

- Risk: a small group controls quorum.
- Mitigation: active set policy, caps, delegation design, monitoring, and
  governance constraints.

Sybil validators:

- Risk: one operator appears as many validators.
- Mitigation: bonding, operational requirements, identity separation, and
  admission rules.

Key compromise:

- Risk: attacker signs consensus messages using validator keys.
- Mitigation: key rotation, key separation, hardware signing, evidence rules,
  and jailing.

Ambiguous validator set:

- Risk: nodes disagree on who may vote.
- Mitigation: deterministic epoch transitions and validator set commitments.

Long-range attack:

- Risk: old validator keys sign alternative history.
- Mitigation: unbonding windows, checkpoints, light-client rules, and historical
  set verification.

Network centralization:

- Risk: only operators with high-end infrastructure can remain active.
- Mitigation: measured requirements, relay non-dependence, performance profiles,
  and conservative active set targets.

Delegation capture:

- Risk: users delegate to a few large operators.
- Mitigation: delegation UX, caps, reward curves, and transparent validator
  metrics.

## Compatibility

Changing validator set derivation, voting power calculation, or epoch transition
rules is a major protocol change.

Adding new validator metadata can be backward-compatible only if:

- the metadata field is versioned
- size limits are defined
- old nodes can ignore it without changing consensus
- activation rules are explicit

Changing consensus key algorithms requires cryptographic identity migration
rules and light-client compatibility analysis.

## Open Decisions

- initial active validator set size policy
- initial voting power model
- minimum validator bond
- delegation support
- stake caps
- validator admission ranking
- epoch length
- activation delay
- deactivation delay
- key rotation delay
- unbonding period
- jailing conditions
- slashing activation
- validator metadata schema
- validator set commitment format
- light-client validator set proof format
- hardware and bandwidth requirements

## Related Specifications

- `docs/rfc/consensus/validator-set.md`
