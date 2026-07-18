# ADR-0019: Storage And State Interfaces

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0008: Block Format
- ADR-0016: Synchronization Checkpoints

Supersedes: None

## Context

HNChain needs persistent storage for blocks, state tree nodes, canonical state
values, receipts, events, validator data, snapshots, indexes, and local node
metadata.

Storage is critical for performance and reliability, but it must not define
consensus truth. Consensus truth is defined by canonical protocol objects,
state transition rules, authenticated state roots, and block commitments.

HNChain may use RocksDB, another embedded database, or a custom storage layer.
The architecture must allow backend replacement without changing block validity.

## Decision

HNChain defines versioned storage and state interfaces.

Conceptual boundary:

```text
Execution Engine
  -> State Access Interface
  -> Authenticated State Tree
  -> Storage Interface
  -> Storage Backend
```

Core interfaces:

- `StateReader`
- `StateWriter`
- `StateTransaction`
- `StateCommitter`
- `BlockStore`
- `ProofStore`
- `SnapshotStore`
- `PruningController`
- `ArchiveStore`

The storage backend stores canonical bytes and implementation indexes. It does
not define state keys, state values, state roots, transaction validity, block
validity, or proof semantics.

## Normative Rules

### Interface Versioning

Every protocol-facing storage interface includes an interface version.

Nodes must not infer storage semantics from database type, column family names,
file layout, path names, or compaction behavior.

### Canonical Bytes At Boundaries

Consensus-relevant objects stored by the backend must be stored or retrievable
as canonical HNCS bytes.

Indexes may use backend-specific encodings, but indexes are not consensus
objects.

### Atomic State Commit

Applying a finalized block must produce an atomic storage commit.

The commit must make the following consistent:

- block header
- block body, if retained
- state root
- state tree nodes
- canonical state values
- receipts, if retained
- events, if retained
- consensus metadata

A node must not expose a partially committed state as valid chain state.

### Deterministic State Access

State reads during execution must be isolated from nondeterministic backend
ordering.

Iteration over state must be explicitly ordered by canonical keys when it
affects consensus behavior.

### Write Set Boundary

Execution produces a deterministic write set.

The storage layer applies that write set through the authenticated state tree.

Storage backends must not reorder consensus writes in a way that changes the
computed state root.

### Rollback And Recovery

Nodes must support recovery from interrupted writes.

Recovery rules must define:

- last fully committed block
- partial commit detection
- state root verification
- write-ahead or equivalent durability mechanism
- rollback window
- corruption handling

### Pruning

Pruning is local retention policy and must not change consensus validity.

A pruned node must not claim archival capabilities.

Pruning profiles must define what data may be discarded and which validation,
sync, and proof services remain supported.

### Archive Storage

Archive nodes retain historical blocks, state proofs, validator set proofs, and
finality proof material according to archival requirements.

Archive service is not consensus authority, but it supports independent audit
and peer synchronization.

### Snapshot Integration

Snapshots must be built from a verified state root.

Snapshot manifests and chunks are storage artifacts, but their validity is
verified against state tree and checkpoint rules.

### Backend Independence

Changing from RocksDB to another backend must not change:

- canonical state keys
- canonical state values
- state root
- block hash
- transaction ID
- receipt root
- event root
- proof verification

## Rejected Options

### Database Layout As Protocol

Rejected because database-specific behavior would make independent
implementations fragile and long-term maintenance risky.

### Unversioned Storage Records

Rejected because storage formats and state schemas must evolve over time.

### Consensus Iteration Over Backend Order

Rejected because database iteration order can differ by backend, version, and
configuration.

### Partial Commit Visible As Chain State

Rejected because it can corrupt state roots and node recovery.

### Archive Requirement For Every Node

Rejected because mandatory archival storage increases node cost and harms
decentralization.

## Alternatives Considered

### RocksDB As Initial Backend

Advantages:

- mature embedded key-value engine
- good operational experience
- useful write batching and compaction features

Disadvantages:

- backend-specific tuning complexity
- compaction and disk behavior require careful operations
- must not leak into consensus semantics

### Custom Storage Engine

Advantages:

- can be tailored to state tree and snapshot requirements
- fewer hidden assumptions if designed carefully

Disadvantages:

- high engineering and audit cost
- long maturity period
- dangerous before workload is well understood

### Pluggable Backend Interface

Advantages:

- supports long-term replaceability
- allows benchmarking multiple backends
- protects consensus from storage implementation details

Disadvantages:

- requires strict interface discipline
- increases testing matrix
- can hide performance cliffs if abstraction is poorly designed

## Security Considerations

State corruption:

- Risk: backend corruption causes invalid state.
- Mitigation: state root verification, atomic commits, recovery checks, and
  corruption detection.

Nondeterministic iteration:

- Risk: different backends produce different execution results.
- Mitigation: canonical key ordering for consensus-relevant iteration.

Partial writes:

- Risk: crash exposes partially applied block state.
- Mitigation: atomic batch commit and recovery metadata.

Snapshot poisoning:

- Risk: node imports false state.
- Mitigation: snapshot root verification against checkpoint or finalized block.

Pruning mistakes:

- Risk: node deletes data needed for validation or proof service.
- Mitigation: pruning profiles and capability declarations.

Index confusion:

- Risk: local indexes are treated as consensus truth.
- Mitigation: indexes remain derived data and must reference canonical objects.

Backend lock-in:

- Risk: one database becomes mandatory by accident.
- Mitigation: interface conformance tests and backend-independent test vectors.

## Compatibility

Changing protocol-facing storage interfaces requires compatibility review.

Backend-specific layout changes can be compatible if:

- canonical objects are unchanged
- migration is deterministic
- state roots are unchanged
- rollback and recovery are specified
- node capability reporting is accurate

Changing state key, state value, or state tree semantics is a protocol change
and is governed by the state tree specification.

## Open Decisions

- initial storage backend
- final storage interface names
- block store schema
- state node storage schema
- receipt and event retention policy
- pruning profiles
- archive requirements
- rollback window
- write-ahead mechanism
- snapshot manifest storage
- corruption recovery policy
- storage conformance test suite
- benchmark methodology

## Related Specifications

- `docs/rfc/storage/storage-state-interfaces.md`
