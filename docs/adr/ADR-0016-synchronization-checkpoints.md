# ADR-0016: Synchronization Checkpoints

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0005: Hash Algorithms
- ADR-0007: State Tree
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0013: Finality Rules
- ADR-0015: Slashing And Accountability

Supersedes: None

## Context

New and recovering nodes need a safe way to synchronize with the network.

Full verification from genesis provides the strongest independent assurance, but
may become expensive as history grows. Checkpoints and snapshots can accelerate
synchronization, but they introduce trust and verification risks if not
specified carefully.

HNChain must define synchronization checkpoints as verifiable consensus objects,
not as trusted files from a website, RPC server, explorer, or foundation.

## Decision

HNChain defines versioned synchronization checkpoints.

Conceptual checkpoint:

```text
SyncCheckpoint
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

Checkpoints summarize finalized history at a specific height and may be used to
bootstrap full nodes, light clients, snapshot verification, and long-range
attack mitigation.

This ADR does not make checkpoints a replacement for finality proofs or full
history verification.

## Normative Rules

### Versioned Checkpoints

Every checkpoint includes `checkpoint_version`.

Nodes must not infer checkpoint format from filename, URL, RPC method, block
height, or client version.

### Finalized Block Binding

A checkpoint must bind to a finalized block.

The checkpoint must include or reference a finality proof that verifies the
checkpoint block under the active consensus profile.

### State Root Binding

A checkpoint must bind to the state root of the checkpoint block.

Snapshots based on the checkpoint must verify against this state root.

### Validator Set Binding

A checkpoint must bind to the validator set or validator set transition data
needed to verify future finality proofs.

Light clients must be able to update validator set state according to explicit
rules.

### Source Independence

Checkpoint validity must not depend on where the checkpoint was downloaded.

Valid sources may include peers, RPC gateways, release artifacts, explorers, or
operator archives, but verification must use canonical proof material.

### Full Verification Mode

HNChain must support full verification from genesis.

Checkpoint-assisted synchronization is an optimization, not the only security
mode.

### Snapshot Integration

Snapshots may be accepted only if their manifest verifies against a checkpoint
or finalized block state root.

Snapshot data without root verification is invalid for consensus state.

### Weak Subjectivity

If the final consensus profile requires weak subjectivity assumptions, the
checkpoint policy must define:

- trusted checkpoint age
- validator set update safety
- offline client risk
- user warning model
- recovery procedure

### Historical Verification

Archive nodes and auditors must be able to verify historical checkpoints using
published specifications and retained proof material.

## Rejected Options

### Trusted Checkpoint File

Rejected because a file distributed by one operator is not consensus truth.

### Explorer Checkpoint As Authority

Rejected because explorer indexes are not consensus objects.

### Snapshot Without State Root Verification

Rejected because it enables state poisoning.

### Mandatory Fast Sync Only

Rejected because full verification from genesis must remain possible.

### Checkpoint As Governance Override

Rejected because checkpoints must summarize finalized history, not rewrite it.

## Alternatives Considered

### Full Verification Only

Advantages:

- strongest independence
- simplest trust model
- excellent for auditors and archive nodes

Disadvantages:

- increasingly expensive as history grows
- slower onboarding for validators and operators
- poor UX for common node recovery

### Checkpoint-Assisted Sync

Advantages:

- faster node onboarding
- useful for weak subjectivity mitigation
- supports practical recovery after downtime

Disadvantages:

- requires explicit trust and proof rules
- can confuse users if checkpoints are treated as authority

### Snapshot-Assisted Sync

Advantages:

- fastest practical full-node recovery
- reduces replay cost
- useful for validators with strict uptime needs

Disadvantages:

- high state poisoning risk without manifest verification
- requires chunking, manifests, and state proof rules
- archival verification remains necessary

### Light-Client Checkpointing

Advantages:

- compact verification for wallets and bridges
- reduces bandwidth and storage requirements

Disadvantages:

- validator set update rules become critical
- weak subjectivity and long-range attacks require careful documentation

## Security Considerations

Checkpoint poisoning:

- Risk: a node accepts a false checkpoint.
- Mitigation: finality proof verification, validator set verification, and hash
  profile binding.

Snapshot poisoning:

- Risk: a node accepts state data that does not match the checkpoint root.
- Mitigation: snapshot manifest and state root verification.

Long-range attack:

- Risk: old validators create an alternative finalized history.
- Mitigation: weak subjectivity policy, checkpoint age limits, unbonding
  windows, and light-client update rules.

Centralization:

- Risk: users trust one official checkpoint server.
- Mitigation: source-independent verification and multiple distribution paths.

Replay across networks:

- Risk: checkpoint is reused on another network.
- Mitigation: chain ID, network ID, consensus profile, and hash profile binding.

Data unavailability:

- Risk: checkpoint exists but required historical data is unavailable.
- Mitigation: archive node requirements and snapshot availability policy.

Operational confusion:

- Risk: operators misunderstand fast sync as full historical verification.
- Mitigation: explicit sync modes and CLI/RPC status reporting.

## Compatibility

Changing checkpoint proof semantics is a major consensus or light-client change.

Adding a checkpoint metadata field can be compatible only if:

- it is versioned
- size limits are defined
- old nodes can ignore it without changing verification
- activation rules are explicit

Snapshot format changes require compatibility analysis with state tree and
storage specifications.

## Open Decisions

- initial checkpoint interval
- checkpoint proof format
- checkpoint metadata schema
- weak subjectivity period
- validator set update proof
- snapshot manifest format
- archive node retention policy
- fast sync mode names
- checkpoint distribution policy
- checkpoint test vectors
- light-client update algorithm
- recovery behavior for conflicting checkpoints

## Related Specifications

- `docs/rfc/consensus/synchronization-checkpoints.md`
