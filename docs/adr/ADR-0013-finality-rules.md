# ADR-0013: Finality Rules

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0005: Hash Algorithms
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0011: Leader Election
- ADR-0012: Vote Messages And Quorum Certificates

Supersedes: None

## Context

Finality defines when a block is considered irreversible under the assumptions
of an accepted consensus profile.

Applications, bridges, wallets, exchanges, and light clients need a precise
definition of finality. A vague finalized flag, local node observation, or block
depth heuristic is not enough for a BFT-oriented protocol with explicit
certificates.

HNChain must define finality as a verifiable consensus condition bound to block
hashes, validator sets, quorum certificates, and protocol versions.

## Decision

HNChain defines finality as a versioned consensus rule that accepts a block only
when a valid finality proof exists for that block under the active consensus
profile.

Conceptual finality proof:

```text
FinalityProof
  proof_version
  consensus_profile
  chain_id
  network_id
  epoch
  height
  round
  block_hash
  validator_set_commitment
  quorum_certificate
  finality_metadata
```

For BFT-oriented profiles, the initial direction is deterministic finality from
one or more quorum certificates over a block or a chain of blocks, depending on
the final consensus algorithm.

This ADR does not accept a final HotStuff, Tendermint, or DAG finality rule.

## Normative Rules

### Explicit Finality Profile

Every finality rule set includes a finality profile or consensus profile.

Nodes must not infer finality semantics from client version, block height,
network name, RPC field names, or local configuration comments.

### Finality Proof Binding

Every finality proof must bind to:

- chain ID
- network ID
- consensus profile
- block hash
- height
- round
- epoch
- validator set commitment
- quorum certificate target
- proof version

Finality proofs valid for one block must not be replayable for another block.

### Block Header Binding

When a finality proof finalizes a block, the proof target must bind to the block
header hash defined by ADR-0008.

Finality must not be based on RPC objects, network packets, body hashes alone,
or local block database identifiers.

### Validator Set Verification

Finality verification must verify that the quorum certificate was produced by
the validator set active for the relevant height, round, and epoch.

Validator set transitions must not create ambiguous finality.

### Quorum Verification

Finality requires quorum certificate verification as defined by ADR-0012.

The finality rule must define whether one certificate is sufficient or whether a
chain of certificates is required.

### Safety Rule

An accepted finality profile must state the safety property it provides.

Minimum required property:

```text
Under stated assumptions, two conflicting blocks at the same height cannot both
be finalized.
```

If the profile provides weaker or probabilistic finality, that must be stated
explicitly.

### Liveness Rule

An accepted finality profile must state its liveness assumptions.

It must define behavior during:

- missing proposer
- invalid proposal
- timeout
- network delay
- validator downtime
- epoch transition
- partial network partition

### Application Semantics

Applications must distinguish:

- seen transaction
- mempool accepted transaction
- proposed block
- certified block
- finalized block

Only finalized blocks are irreversible under the finality profile assumptions.

### Light-Client Verification

Light clients must verify finality proofs using canonical block headers,
validator set commitments, quorum certificates, and hash profiles.

Light clients must not trust an RPC `finalized: true` flag without proof.

### Checkpoints

Checkpoints may summarize finalized history, but checkpoint rules must define
how the checkpoint is produced, verified, and linked to finality proofs.

Checkpoints must not silently replace finality rules.

## Rejected Options

### Confirmation Depth As Finality

Rejected for the initial BFT-oriented direction because block depth is a
probabilistic heuristic, not an explicit finality proof.

### RPC Finalized Flag

Rejected because RPC is not consensus truth.

### Local Node Observation

Rejected because one node seeing a block does not prove network finality.

### Finality Without Validator Set Binding

Rejected because certificates cannot be trusted without knowing who was allowed
to vote and how much voting power they represented.

### Finality Rule Hidden In Implementation

Rejected because multiple implementations must be able to verify the same
finality proof from specifications.

## Alternatives Considered

### Tendermint-Style Two-Phase Finality

Advantages:

- clear prevote/precommit mental model
- deterministic finality
- mature operational experience

Disadvantages:

- communication overhead grows with validator set size
- latency depends on network and timeout tuning

### HotStuff-Style Chained Finality

Advantages:

- pipeline-friendly
- compact quorum-certificate oriented model
- good fit for block justification design

Disadvantages:

- finality rule is subtler
- requires careful view-change and timeout design
- implementation and testing complexity are higher

### DAG-Based Finality

Advantages:

- can separate data dissemination from ordering/finality
- may improve throughput under high load

Disadvantages:

- significantly more complex
- harder light-client and bridge verification
- data availability assumptions become central

### Probabilistic Finality

Advantages:

- may scale in some consensus families
- can be simple for clients that tolerate probability

Disadvantages:

- less aligned with HNChain's deterministic finality direction
- harder for bridges, exchanges, and high-value settlement

## Security Considerations

Conflicting finality:

- Risk: two conflicting blocks finalize at the same height.
- Mitigation: quorum intersection, validator set verification, signing context,
  and evidence rules.

Long-range attack:

- Risk: old validators create alternative finality proofs.
- Mitigation: checkpoint policy, unbonding windows, validator set history, and
  light-client update rules.

Weak subjectivity:

- Risk: clients joining after long offline periods need a trusted recent
  checkpoint or social reference.
- Mitigation: explicit light-client and checkpoint policy.

Certificate replay:

- Risk: a valid certificate is reused for another chain, network, height, or
  block.
- Mitigation: finality proof context binding.

Data unavailability:

- Risk: a block is finalized before enough validators can verify body data.
- Mitigation: consensus profile must define availability checks before voting.

Ambiguous epoch transition:

- Risk: clients disagree which validator set finalized a block.
- Mitigation: deterministic epoch transition and validator set commitment rules.

Overstated guarantees:

- Risk: users treat finality as absolute even outside stated assumptions.
- Mitigation: documentation must state assumptions and failure modes.

## Compatibility

Changing finality rules is a major consensus change.

Adding a new finality proof format can be compatible only if:

- proof version is defined
- old nodes reject unsupported proofs deterministically
- activation rules are explicit
- light-client migration rules are specified

Changing checkpoint rules requires compatibility analysis for light clients,
bridges, exchanges, and archival services.

## Open Decisions

- initial finality profile
- one-QC versus chained-QC finality
- final vote types used for finality
- final quorum threshold formula
- timeout certificate interaction
- nil vote finality behavior
- epoch transition finality rule
- checkpoint interval
- weak subjectivity policy
- light-client update rule
- data availability precondition
- rollback and halt behavior
- finality proof inclusion in block justification
- finality proof maximum size

## Related Specifications

- `docs/rfc/consensus/finality-rules.md`
- `docs/rfc/consensus/fork-choice-rules.md`
