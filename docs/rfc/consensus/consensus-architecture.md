# HNChain Consensus RFC: Architecture

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0010-validator-set-model.md`

## 1. Purpose

This RFC defines the architecture boundary for HNChain consensus.

It does not finalize the consensus algorithm. It defines the modules,
interfaces, safety requirements, and verification surfaces that any accepted
consensus profile must satisfy.

## 2. Goals

- Deterministic finality under explicit assumptions.
- No forks in normal operation after finality.
- Byzantine fault tolerance with documented threshold.
- Modular leader selection, ordering, voting, and finality.
- Verifiable block justification for nodes and light clients.
- Explicit evidence and accountability model.
- Compatibility with future consensus profile upgrades.

## 3. Non-Goals

- Define final staking economics.
- Define final validator count.
- Define final slashing amounts.
- Define a final networking transport.
- Claim achieved TPS or finality latency before implementation and testing.

## 4. Consensus Components

Conceptual component graph:

```text
Transaction Pool
  -> Proposal Builder
  -> Block Proposal
  -> Proposal Validator
  -> Voting
  -> Quorum Certificate
  -> Finality
  -> State Commit
```

Supporting components:

```text
Validator Set
Leader Election
Evidence Pool
Checkpoint Manager
Sync Verifier
Light Client Verifier
```

## 5. Module Responsibilities

### 5.1 Validator Set

Defines the active validators and voting power for a height or epoch.

Required outputs:

- validator identity
- voting power
- key descriptors
- activation height or epoch
- deactivation height or epoch

### 5.2 Leader Election

Selects proposer candidates according to the active consensus profile.

Required properties:

- deterministic verification
- resistance to manipulation
- defined fallback on leader failure
- clear interaction with epochs

### 5.3 Proposal Builder

Builds candidate blocks from locally available transactions and consensus data.

Proposal builder policy may vary by implementation, but the resulting block must
be valid under consensus rules.

### 5.4 Proposal Validator

Verifies proposed block structure before voting.

Required checks:

- block version
- chain and network identifiers
- parent hash
- proposer eligibility
- size limits
- committed roots
- basic transaction validity
- data availability, if required before voting

### 5.5 Voting

Verifies and records consensus votes.

Vote messages must bind to:

- chain ID
- network ID
- consensus profile
- height
- round
- epoch
- block hash
- vote type
- validator identity

### 5.6 Quorum Certificate

Aggregates enough votes to prove that a quorum accepted a consensus claim.

The quorum certificate format must define:

- threshold
- signer set
- voting power calculation
- signature verification
- duplicate signer handling
- malformed certificate rejection

### 5.7 Finality

Determines when a block is final.

Finality must produce or verify a `BlockJustification` compatible with the block
format specification.

### 5.8 Evidence

Collects and validates Byzantine evidence.

Initial evidence categories:

- double proposal
- double vote
- invalid vote signature
- conflicting finality proof

Penalty semantics are outside this RFC until staking and validator economics are
specified.

### 5.9 Checkpoints

Checkpoints help node synchronization and long-range attack resistance.

Checkpoint rules must define:

- checkpoint interval
- checkpoint commitment
- signing or finality proof
- light-client verification behavior
- rollback implications

### 5.10 Synchronization

Synchronization verifies history or state against consensus proofs.

Supported conceptual modes:

- full verification from genesis
- checkpoint-assisted verification
- snapshot-assisted verification
- light-client verification

## 6. Consensus Messages

Final message schemas are open.

Minimum conceptual message classes:

```text
Proposal
Vote
QuorumCertificate
Timeout
Evidence
Checkpoint
SyncRequest
SyncResponse
```

Every network message must have:

- message version
- consensus profile
- chain ID
- network ID
- payload type
- bounded payload
- canonical encoding
- verification rules

## 7. Safety Requirements

An accepted consensus profile must prove or justify:

- conflicting blocks cannot both finalize under stated assumptions
- finalized block history has a single canonical state root per height
- validator set changes cannot create ambiguous voting power
- old signatures cannot be replayed into new heights, rounds, epochs, or chains
- light clients can verify finality without trusting RPC responses

## 8. Liveness Requirements

An accepted consensus profile must define:

- network timing assumptions
- timeout behavior
- leader failure behavior
- recovery from temporary partitions
- behavior when less than quorum is online
- behavior during validator set changes

Liveness claims must include failure modes.

## 9. Block Format Interaction

Consensus writes into block format fields:

```text
round
epoch
proposer
consensus_root
evidence_root
justification
```

Consensus verifies:

```text
parent_block_hash
transactions_root
state_root
receipts_root
events_root
protocol_parameters_hash
```

Consensus must not mutate state outside the state transition rules.

## 10. Security Requirements

Implementations must reject:

- unknown consensus profiles
- invalid proposer identities
- invalid vote contexts
- duplicate vote signatures
- quorum certificates below threshold
- certificates with incorrect voting power
- finality proofs for another chain or network
- evidence with malformed canonical bytes
- oversized consensus messages
- messages from unsupported protocol versions

Implementations must bound:

- message decode cost
- signature verification batch size
- vote cache memory
- evidence verification time
- sync response size
- checkpoint proof size

## 11. Test And Verification Requirements

Before production implementation, consensus must have:

- deterministic state-machine tests
- adversarial message-order tests
- duplicate vote tests
- equivocation tests
- timeout and leader failure tests
- network partition simulations
- validator set transition tests
- light-client finality proof tests
- long-range attack scenario tests
- reproducible benchmark profile

Formal modeling is required before any novel consensus variant is accepted.

## 12. Open Decisions

- initial consensus profile
- final consensus message schemas
- final quorum certificate format
- final validator set format
- final timeout model
- final checkpoint model
- final evidence model
- final light-client proof format
- final data availability rule
- final performance benchmark suite
