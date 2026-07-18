# HNChain Consensus RFC: Synchronization Checkpoints

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0013-finality-rules.md`
- `docs/adr/ADR-0015-slashing-and-accountability.md`
- `docs/adr/ADR-0016-synchronization-checkpoints.md`

## 1. Purpose

This RFC defines the conceptual synchronization checkpoint model for HNChain.

It specifies checkpoint structure, verification inputs, sync modes, snapshot
integration, weak subjectivity hooks, and security requirements.

## 2. Scope

This RFC defines:

- checkpoint object requirements
- checkpoint verification requirements
- sync mode boundaries
- snapshot verification relationship
- weak subjectivity hooks
- archive and audit expectations

This RFC does not define:

- final snapshot chunk format
- final storage backend layout
- final weak subjectivity period
- final checkpoint interval
- final P2P sync packet format
- final CLI commands

## 3. Checkpoint Object

Conceptual structure:

```text
SyncCheckpointV1
  checkpoint_version
  chain_id
  network_id
  consensus_profile
  checkpoint_height
  checkpoint_block_hash
  checkpoint_state_root
  validator_set_commitment
  finality_proof
  checkpoint_metadata
```

All fields that affect verification must be HNCS-encoded.

## 4. Sync Modes

Conceptual sync modes:

```text
full_from_genesis
checkpoint_assisted
snapshot_assisted
light_client
archive
```

Mode semantics:

- `full_from_genesis`: verifies history from genesis.
- `checkpoint_assisted`: verifies a checkpoint, then continues from that point.
- `snapshot_assisted`: verifies checkpoint and snapshot state root, then
  continues.
- `light_client`: verifies headers, validator set updates, and finality proofs.
- `archive`: retains historical data and proofs for audit and serving peers.

## 5. Checkpoint Verification

Verification requires:

- supported checkpoint version
- matching chain ID and network ID
- supported consensus profile
- canonical checkpoint block hash
- canonical checkpoint state root
- valid finality proof
- valid validator set commitment
- supported hash profiles
- bounded metadata

Verification must not depend on checkpoint download source.

## 6. Snapshot Verification

Snapshot-assisted sync requires:

- checkpoint state root
- snapshot manifest
- chunk hashes
- state tree profile
- hash profile
- final reconstructed state root

The reconstructed state root must equal the checkpoint state root.

## 7. Validator Set Updates

Checkpoint verification must provide enough information for future finality
proof verification.

Validator set update rules must define:

- current validator set proof
- next validator set commitment
- transition height or epoch
- light-client update rule
- invalid transition handling

## 8. Weak Subjectivity

If required by the final consensus profile, weak subjectivity policy must
define:

- maximum safe offline duration
- trusted checkpoint age
- user warning behavior
- validator set change assumptions
- recovery process
- source diversity recommendations

Weak subjectivity assumptions must be documented plainly.

## 9. Archive Expectations

Archive nodes support:

- historical block retrieval
- historical finality proof retrieval
- historical validator set proof retrieval
- checkpoint proof retrieval
- snapshot verification assistance

Archive service is not consensus authority, but it improves independent audit.

## 10. RPC And CLI Reporting

Node APIs and CLI should report sync mode clearly.

Examples:

```text
sync_mode: full_from_genesis
sync_mode: checkpoint_assisted
sync_mode: snapshot_assisted
```

Clients must be able to distinguish full historical verification from
checkpoint-assisted verification.

## 11. Security Requirements

Implementations must reject:

- unknown checkpoint versions
- checkpoints for another chain or network
- checkpoints with invalid finality proofs
- checkpoints with mismatched block hash
- checkpoints with mismatched state root
- checkpoints with invalid validator set commitment
- snapshots with mismatched reconstructed root
- oversized checkpoint metadata
- unsupported hash profiles
- conflicting checkpoints without recovery policy

Implementations must bound:

- checkpoint decode cost
- finality proof verification cost
- validator set proof size
- snapshot manifest size
- snapshot chunk size
- checkpoint metadata size

## 12. Test Vectors

The accepted version must include test vectors for:

- valid checkpoint
- wrong chain rejection
- wrong network rejection
- invalid finality proof rejection
- mismatched block hash rejection
- mismatched state root rejection
- invalid validator set commitment rejection
- snapshot root verification
- conflicting checkpoint detection
- unsupported checkpoint version rejection

Test vectors are mandatory before production implementation.

## 13. Open Decisions

- final checkpoint schema
- final checkpoint interval
- final weak subjectivity period
- final validator set update proof
- final snapshot manifest schema
- final snapshot chunk format
- final archive node requirements
- final sync mode API names
- final P2P checkpoint exchange format
- final checkpoint recovery procedure
