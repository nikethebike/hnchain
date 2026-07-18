# HNChain Storage RFC: Storage And State Interfaces

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0001-account-state-model.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0016-synchronization-checkpoints.md`
- `docs/adr/ADR-0019-storage-state-interfaces.md`

## 1. Purpose

This RFC defines the conceptual storage and state interface model for HNChain.

It specifies module boundaries, interface responsibilities, atomic commit
requirements, pruning boundaries, snapshot integration, and backend
independence.

## 2. Scope

This RFC defines:

- protocol-facing storage boundaries
- state access interfaces
- atomic commit requirements
- rollback and recovery requirements
- pruning profile requirements
- archive node expectations
- backend conformance requirements

This RFC does not define:

- final RocksDB schema
- final custom storage engine
- final column family layout
- final compaction tuning
- final snapshot chunk format
- final benchmark values

## 3. Module Boundary

Conceptual architecture:

```text
Consensus
  -> Block Execution
  -> State Access Interface
  -> Authenticated State Tree
  -> Storage Interface
  -> Backend Adapter
  -> Physical Storage
```

Only canonical protocol objects and authenticated commitments define consensus
truth.

## 4. Interfaces

Conceptual interfaces:

```text
StateReader
StateWriter
StateTransaction
StateCommitter
BlockStore
ProofStore
SnapshotStore
PruningController
ArchiveStore
```

Final language-specific traits or interfaces are not defined by this RFC.

## 5. StateReader

`StateReader` provides deterministic reads against a known state root.

Required behavior:

- read by canonical state key
- verify state root context
- return canonical state value bytes
- reject unknown state versions
- expose ordered iteration only when explicitly supported

## 6. StateWriter

`StateWriter` records deterministic state changes produced by execution.

Required behavior:

- accept canonical state keys
- accept canonical state values
- reject malformed values before commit
- record deletions or lifecycle changes explicitly
- produce a deterministic write set

## 7. StateTransaction

`StateTransaction` isolates execution writes until commit or rollback.

Required behavior:

- begin from a known state root
- expose read-your-writes behavior when required
- prevent partial commit visibility
- support rollback before commit
- produce commit metadata

## 8. StateCommitter

`StateCommitter` applies a write set to the authenticated state tree.

Required output:

```text
StateCommitResult
  previous_state_root
  next_state_root
  state_version
  tree_profile
  write_set_hash
  commit_metadata
```

`next_state_root` must match the state tree specification.

## 9. BlockStore

`BlockStore` persists canonical block data.

Required capabilities:

- store canonical block header
- store canonical block body, if retention profile requires it
- retrieve by block hash
- retrieve by height according to canonical chain view
- store finality proof material
- record finalized height

Local indexes may accelerate lookup but must remain derived data.

## 10. ProofStore

`ProofStore` stores or reconstructs proof material.

Proof material may include:

- state proofs
- validator set proofs
- finality proofs
- checkpoint proofs
- light-client proof components

Proof validity is defined by protocol specifications, not by storage indexes.

## 11. SnapshotStore

`SnapshotStore` stores snapshot manifests and chunks.

Snapshot data must verify against:

- state root
- state tree profile
- hash profile
- checkpoint or finalized block

Unverified snapshots must not be exposed as chain state.

## 12. PruningController

`PruningController` enforces local retention policy.

Conceptual profiles:

```text
archive
full_recent
validator_recent
light
```

Each profile must define:

- retained block range
- retained state range
- retained proof material
- rollback support
- peer-serving capabilities
- operator warnings

## 13. Atomic Commit

Committing a block must be atomic with respect to canonical chain state.

Conceptual commit:

```text
begin
  -> store block header
  -> store block body according to retention policy
  -> apply state write set
  -> store state tree nodes
  -> store receipts and events according to retention policy
  -> store finality and consensus metadata
  -> mark commit complete
```

After crash recovery, a node must resume from the last complete commit or roll
back incomplete data.

## 14. Backend Adapter

Backend adapters may use backend-specific layouts.

They must provide conformance for:

- canonical object retrieval
- atomic commit behavior
- crash recovery
- deterministic ordered reads when required
- corruption detection
- pruning behavior
- snapshot import and export

## 15. Security Requirements

Implementations must reject:

- unknown protocol-facing interface versions
- non-canonical state keys
- non-canonical state values
- malformed state tree nodes
- commits with mismatched previous state root
- commits with incorrect next state root
- partially committed chain state
- snapshots with mismatched state root
- archive claims without retained data

Implementations must bound:

- write set size
- commit batch size
- state value size
- proof reconstruction time
- snapshot chunk size
- recovery scan time
- pruning work per cycle

## 16. Test Vectors

The accepted version must include test vectors for:

- read missing key
- write single key
- deterministic write set hash
- commit from empty state
- commit with previous root mismatch
- rollback before commit
- crash recovery after partial commit
- pruning profile behavior
- snapshot root verification
- backend conformance

Test vectors are mandatory before production implementation.

## 17. Open Decisions

- final interface definitions
- final backend adapter API
- initial storage backend
- final block store layout
- final state node layout
- final proof store layout
- final pruning profiles
- final rollback window
- final snapshot manifest schema
- final recovery metadata schema
- final storage benchmark suite
