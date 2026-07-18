# ADR-0011: Leader Election

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model

Supersedes: None

## Context

Leader election selects the validator that may propose a block for a consensus
height, round, slot, or equivalent unit.

Poor leader election can create censorship, denial-of-service concentration,
predictable proposer targeting, stake grinding, unfair rewards, and liveness
failures.

HNChain requires a leader election model that is deterministic to verify,
resistant to manipulation, compatible with large validator participation, and
separable from the final consensus algorithm.

## Decision

HNChain defines leader election as a versioned consensus module.

Conceptual inputs:

```text
LeaderElectionInput
  election_version
  consensus_profile
  chain_id
  network_id
  epoch
  height
  round
  validator_set_commitment
  randomness_commitment
  protocol_parameters_hash
```

Conceptual output:

```text
LeaderElectionOutput
  proposer
  proof
  priority_metadata
```

The initial direction is verifiable proposer selection from the active validator
set using a protocol-defined randomness source or deterministic rotation with
documented fairness and attack-resistance trade-offs.

This ADR does not accept a final randomness scheme, VRF construction, committee
selection model, or proposer priority algorithm.

## Normative Rules

### Versioned Election Profile

Every leader election rule set includes an election version or profile.

Nodes must not infer election behavior from client version, validator software,
network topology, wall-clock time, or mempool state.

### Deterministic Verification

Given the same canonical inputs, all honest nodes must verify the same eligible
proposer for a height and round.

Leader verification must use:

- canonical validator set
- canonical epoch and height
- canonical round
- canonical randomness or rotation state
- canonical protocol parameters

### Validator Set Binding

Leader election operates only over the active validator set defined by
ADR-0010.

Inactive, jailed, exited, or otherwise ineligible validators must not be
selected.

### Randomness Source

If randomness is used, it must be protocol-defined, publicly verifiable, and
domain-separated.

The randomness mechanism must define:

- source
- lifecycle
- entropy assumptions
- bias resistance
- grinding resistance
- reveal timing
- fallback on missing reveals
- light-client verification behavior

Local node randomness is not a consensus input.

### Lookahead

Leader predictability window must be explicitly defined.

Long lookahead improves operational preparation but increases targeted attack
risk.

Short lookahead reduces targeted attack surface but may increase networking and
liveness complexity.

### Fairness

The election profile must define fairness relative to the chosen voting power
model.

Fairness may mean equal proposer frequency, voting-power-weighted frequency,
capped weighted frequency, or committee-specific probability.

The chosen model must be measurable.

### Liveness

Leader election must define fallback behavior when a proposer is unavailable,
late, equivocal, or produces an invalid block.

Fallback must not depend on local wall-clock time unless the consensus profile
defines deterministic timeout validation rules.

### Proposer Proof

If leader eligibility requires a proof, the proof must bind to:

- chain ID
- network ID
- consensus profile
- election profile
- validator identity
- epoch
- height
- round
- validator set commitment
- randomness commitment, if used

### Mempool Independence

Mempool contents, transaction arrival order, and local fee policy must not
affect who is eligible to propose.

### Upgrade Compatibility

Changing leader election rules is a consensus profile change unless explicitly
defined as a parameter update.

## Rejected Options

### Static Ordered Validator List Forever

Rejected because a permanently predictable schedule increases targeted attack
risk and creates poor adaptability.

Static rotation may still be used as a simple baseline for devnet or early
testing if it is clearly marked non-production.

### Local Randomness

Rejected because validators would compute different leaders.

### Mempool-Based Leader Election

Rejected because mempool state is local and adversarially influenceable.

### Unverifiable Off-Chain Randomness

Rejected because nodes and light clients must verify proposer eligibility
without trusting an operator or service.

### Immediate Reselection Without Rules

Rejected because ambiguous fallback behavior can create competing proposals and
liveness instability.

## Alternatives Considered

### Round-Robin Rotation

Advantages:

- simple
- easy to test
- no randomness dependency
- predictable operations

Disadvantages:

- predictable proposer schedule
- vulnerable to targeted denial of service
- weak fairness if validator set changes frequently

### Weighted Round-Robin

Advantages:

- aligns proposer frequency with voting power
- deterministic and auditable
- avoids external randomness

Disadvantages:

- predictable schedule
- more complex with dynamic voting power
- may amplify stake concentration

### VRF-Based Selection

Advantages:

- eligibility can be privately known until reveal
- proof is publicly verifiable
- useful for reducing targeted proposer attacks

Disadvantages:

- adds cryptographic complexity
- requires careful key lifecycle and proof encoding
- grinding and withholding rules must be specified

### Random Beacon Based Selection

Advantages:

- common randomness can support committees and proposer selection
- compatible with light-client verification if committed in consensus

Disadvantages:

- beacon construction is difficult
- missing contribution and bias resistance require careful design
- can become a liveness bottleneck

### Committee-Based Proposer Selection

Advantages:

- can reduce load for very large validator sets
- may improve scalability of voting and proposal dissemination

Disadvantages:

- committee randomness becomes safety-critical
- more complex fairness and light-client proofs
- additional censorship and availability risks

## Security Considerations

Targeted proposer attack:

- Risk: attackers disrupt known future leaders.
- Mitigation: bounded lookahead, verifiable randomness, fallback rules, and
  network redundancy.

Grinding:

- Risk: validators manipulate randomness or validator set state to improve
  proposer probability.
- Mitigation: domain-separated randomness, delayed activation, reveal rules, and
  bias analysis.

Censorship:

- Risk: repeated proposer control censors transactions.
- Mitigation: fairness metrics, leader rotation, fallback, and future inclusion
  rules.

Equivocation:

- Risk: a proposer creates conflicting blocks for the same height and round.
- Mitigation: signed proposal context and canonical evidence.

Liveness failure:

- Risk: unavailable leaders stall the network.
- Mitigation: deterministic timeout, round change, and alternate proposer rules.

Stake concentration:

- Risk: weighted selection gives large operators frequent proposer control.
- Mitigation: capped weighting, delegation policy, monitoring, or alternative
  fairness models.

Randomness failure:

- Risk: randomness becomes biased, unavailable, or unverifiable.
- Mitigation: fallback rules, explicit beacon lifecycle, and light-client proof
  requirements.

## Compatibility

Leader election profile changes require explicit activation rules.

Adding a new randomness source can be backward-compatible only if:

- the source is versioned
- old nodes reject unsupported profiles deterministically
- commitments are included in consensus metadata
- light-client verification rules are defined

Changing proposer eligibility for an active epoch is a major protocol risk and
requires migration analysis.

## Open Decisions

- initial election profile
- round semantics
- proposer lookahead window
- randomness source
- VRF algorithm, if any
- random beacon design, if any
- weighted versus equal proposer probability
- proposer priority tie-breaking
- fallback proposer selection
- timeout interaction
- proposer proof format
- randomness commitment format
- leader schedule proof for light clients
- grinding resistance analysis
- committee selection relationship

## Related Specifications

- `docs/rfc/consensus/leader-selection.md`
