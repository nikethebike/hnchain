# ADR-0009: Consensus Architecture

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0008: Block Format

Supersedes: None

## Context

Consensus is the mechanism by which HNChain nodes agree on block history and the
resulting state root in the presence of network delay, faults, and Byzantine
behavior.

HNChain targets fast finality and high throughput, but these are engineering
targets, not accepted safety guarantees. The consensus design must prioritize
correctness, explicit assumptions, and long-term maintainability over headline
performance claims.

The protocol must avoid coupling leader selection, transaction ordering,
finality, validator accounting, network transport, and storage internals into a
single inseparable implementation.

## Decision

HNChain defines consensus as a modular subsystem composed of replaceable
protocol roles and interfaces.

Conceptual architecture:

```text
Consensus Engine
  -> Validator Set
  -> Leader Election
  -> Proposal Validation
  -> Transaction Ordering
  -> Voting
  -> Finality
  -> Evidence
  -> Checkpoints
  -> Synchronization
```

The first consensus profile should be BFT-oriented and should target safety when
fewer than one third of active voting power is Byzantine, assuming the final
algorithm satisfies its network and timing assumptions.

This ADR does not accept a final algorithm. HotStuff-style BFT, Tendermint-style
BFT, DAG-based mempool plus BFT finality, and other candidates remain under
evaluation.

## Normative Rules

### Explicit Safety Model

Every accepted consensus profile must define:

- fault threshold
- voting power model
- quorum threshold
- synchrony or partial synchrony assumptions
- finality rule
- fork-choice rule, if any
- validator set update rule
- evidence and accountability rule
- liveness assumptions

Claims about fixed finality latency are invalid unless the assumptions and
failure modes are documented.

### Modular Boundaries

Consensus modules must have explicit interfaces.

Required conceptual modules:

- validator set management
- leader or proposer selection
- proposal verification
- transaction ordering
- vote verification
- quorum certificate construction
- finality verification
- evidence verification
- checkpoint verification
- catch-up synchronization

Replacing one module must not require redefining unrelated protocol objects.

### Block Format Integration

Consensus uses the block format defined by ADR-0008.

Consensus-specific data must be committed through:

- `round`
- `epoch`
- `proposer`
- `consensus_root`
- `evidence_root`
- `justification`

Consensus must not add hidden validity rules through network packet metadata or
local node state.

### Validator Identity

Validators are identified through canonical cryptographic identity descriptors.

Validator signatures must bind to:

- protocol name
- chain ID
- network ID
- consensus profile
- height
- round
- epoch
- block hash or consensus object hash
- signing purpose

Signature verification must use the cryptographic identity specification.

### Finality

Finality must be represented by an explicit proof or certificate.

The proof must be verifiable by nodes and light clients according to the
accepted consensus profile.

Applications must not treat gossip reception, mempool inclusion, or block
proposal as finality.

### Transaction Ordering

Consensus defines the canonical order of transactions in accepted blocks.

Mempool ordering is local policy unless explicitly promoted into consensus
rules.

The final block order must produce deterministic execution and state roots.

### Leader Election

Leader or proposer selection must be specified independently from block
validation.

If randomness is used, it must come from a protocol-defined source and must be
verifiable.

Leader selection must define resistance to targeted attacks, grinding, stake
concentration, and denial-of-service amplification.

### Evidence And Accountability

Consensus must define evidence formats before activating penalties such as
slashing.

Evidence must be canonical, bounded, and verifiable.

Punishment rules must be deterministic and must not depend on discretionary
operator judgment.

### Synchronization

Consensus must define how new or recovering nodes safely catch up.

Required modes:

- full history verification
- checkpoint-assisted synchronization
- snapshot-assisted synchronization, if snapshots are activated
- light-client verification

Fast synchronization must not require trusting arbitrary RPC responses.

### Protocol Upgrades

Consensus upgrades must define:

- activation condition
- activation height or epoch
- validator signaling requirements, if any
- compatibility behavior before activation
- rollback and halt behavior
- light-client impact

Changing finality rules is a major protocol change.

## Rejected Options

### Proof Of Work

Rejected for HNChain's initial direction because it conflicts with the stated
goals of fast finality and low energy consumption.

This does not mean Proof of Work is insecure. It means it does not match
HNChain's intended design constraints.

### Undocumented Hybrid Consensus

Rejected because combining ideas from multiple consensus families without a
formal safety model creates hidden failure modes.

### Fixed Small Validator Set As A Shortcut

Rejected as a default assumption because it can improve performance by reducing
decentralization.

A bounded active set may still be selected if justified by safety, performance,
and governance analysis.

### Mempool Order As Consensus

Rejected because mempool contents and arrival order differ across nodes.

### Local Time As Consensus Randomness

Rejected because local clocks are not deterministic across validators.

## Alternatives Considered

### Tendermint-Style BFT

Advantages:

- mature conceptual model
- clear safety threshold
- deterministic finality
- simpler reasoning than many highly optimized protocols

Disadvantages:

- communication overhead grows with validator set size
- very large validator sets are difficult without aggregation or committee
  mechanisms

### HotStuff-Style BFT

Advantages:

- pipeline-friendly design
- quorum certificates provide compact finality evidence
- strong fit for modular finality proofs

Disadvantages:

- implementation complexity
- leader failure and timeout tuning are subtle
- performance depends heavily on networking and signature aggregation

### DAG-Based Ordering With BFT Finality

Advantages:

- can separate data dissemination from finality
- may improve throughput under high load
- can reduce leader bottlenecks

Disadvantages:

- significantly more complex
- harder to audit
- finality and data availability interactions require careful proof

### Avalanche-Style Metastable Consensus

Advantages:

- scalable sampling-based approach
- useful design ideas for fast probabilistic agreement

Disadvantages:

- probabilistic finality semantics may not match HNChain's deterministic
  finality goal
- safety analysis differs from classical BFT assumptions

## Security Considerations

Safety violation:

- Risk: two conflicting blocks finalize at the same height.
- Mitigation: formal quorum rules, slashing evidence, signature binding, and
  consensus test vectors.

Liveness failure:

- Risk: the network stops finalizing blocks even without safety failure.
- Mitigation: explicit timeout, view-change, leader rotation, and network
  assumptions.

Long-range attacks:

- Risk: old validators create an alternative finalized history.
- Mitigation: checkpoint rules, unbonding windows, light-client security model,
  and historical validator set verification.

Targeted proposer attacks:

- Risk: attackers disrupt known future leaders.
- Mitigation: verifiable unpredictability, short lookahead, redundancy, or
  proposer rotation design.

Vote equivocation:

- Risk: validators sign conflicting consensus messages.
- Mitigation: signing context, evidence root, deterministic penalties, and
  operator key isolation.

Data unavailability:

- Risk: votes finalize a block whose body is unavailable.
- Mitigation: availability checks before voting and explicit propagation rules.

Stake centralization:

- Risk: high stake concentration becomes consensus capture.
- Mitigation: staking economics, delegation design, validator set policy, and
  governance limits.

Complexity risk:

- Risk: a novel consensus design contains subtle correctness bugs.
- Mitigation: conservative module boundaries, formal modeling, simulation,
  independent review, and staged testnets.

## Compatibility

Consensus profile changes are major protocol events unless explicitly designed
as compatible parameter updates.

Adding a new consensus message can be backward-compatible only if:

- message type is versioned
- network packet format is specified
- unsupported nodes reject it deterministically
- activation rules are defined
- finality verification remains unambiguous

Changing validator set rules, quorum thresholds, or finality semantics requires
explicit migration planning.

## Open Decisions

- initial consensus family
- active validator set selection
- voting power model
- quorum threshold
- signature aggregation scheme
- leader selection randomness source
- timeout and view-change rules
- epoch length
- validator set update timing
- slashing activation model
- evidence format
- checkpoint interval
- light-client finality proof
- data availability rule
- consensus networking channels
- performance benchmark methodology
- formal modeling framework

## Related Specifications

- `docs/rfc/consensus/consensus-architecture.md`
- `docs/rfc/consensus/validator-set.md`
