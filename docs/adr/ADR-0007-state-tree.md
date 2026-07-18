# ADR-0007: State Tree

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format

Supersedes: None

## Context

HNChain uses an extended account-based state model. Every accepted block must
commit to the exact state that results from applying an ordered set of
deterministic state transitions.

The state tree defines this commitment.

The storage engine may use RocksDB, another embedded database, or a custom
storage layer, but the database layout must not define consensus truth. Consensus
truth is defined by canonical state keys, canonical state values, hash profiles,
tree rules, and the resulting state root.

## Decision

HNChain defines a versioned authenticated state tree abstraction.

Conceptual structure:

```text
StateCommitment
  state_version
  tree_profile
  hash_profile
  height
  state_root
```

The initial implementation candidate is a sparse authenticated tree profile with
fixed-length canonical state keys.

The exact initial tree profile remains open until benchmark and proof-size
analysis are complete. The protocol must nevertheless expose a stable
`StateCommitment` abstraction so block format, snapshots, proofs, and light
clients do not depend on a specific database implementation.

## Normative Rules

### Versioned State Commitment

Every state commitment includes `state_version`.

Nodes must not infer state format from root length, database schema, block
height, or implementation version.

### Canonical State Keys

Every state object is addressed by a canonical state key.

Conceptual derivation:

```text
state_key = STATE_KEY(domain, object_id, subkey)
```

The final derivation must define:

- domain registry
- object identifier encoding
- subkey encoding
- key length
- domain separation string
- hash profile
- collision handling rules

State keys must not depend on JSON field order, local database ordering, memory
layout, filesystem paths, or host endianness.

### State Domains

The state tree must support independent domains.

Initial conceptual domains:

- accounts
- account_extensions
- contracts
- contract_storage
- assets
- validators
- governance
- metadata
- system

Domains are part of the key derivation and proof semantics.

### Canonical State Values

Every committed value is encoded using HNCS.

Every committed value includes or references a schema version.

Hashing a value means hashing its canonical HNCS bytes under the active state
value hash profile.

### State Root

The state root is computed only from canonical tree nodes.

Two honest nodes that apply the same ordered transitions to the same previous
state root must compute the same next state root.

### Node Types

The tree profile must define all node types explicitly.

Conceptual node classes:

- empty node
- leaf node
- internal node
- compressed path node, if supported by the selected profile

Each node type must have a domain-separated hash encoding.

### Updates

Block execution updates the state tree by applying validated transaction effects
in deterministic block order.

Parallel execution is allowed only if the final commit order is deterministic
and produces the same root as the canonical serial execution rule.

### Proofs

The state tree must support inclusion and non-inclusion proofs.

Proofs must define:

- proof version
- tree profile
- hash profile
- queried key
- root being proven against
- node sequence or equivalent commitment data
- size limits
- verification rules

Light clients, wallets, bridges, and explorers must verify proofs against block
headers rather than trusted RPC responses.

### Snapshots

Snapshots must commit to a state root and the metadata needed to verify that
root.

A snapshot is valid only if its manifest is authenticated by consensus-approved
checkpoint or block data.

Snapshot format is separate from the state tree profile.

### Pruning And Archival Storage

Pruning is a local storage policy and must not change consensus validity.

Archive nodes retain historical state data. Pruned nodes may discard old tree
nodes only when they can still validate the required consensus window and serve
their declared node capabilities.

### Storage Backend Independence

RocksDB or any other storage backend stores tree nodes and state values. It does
not define:

- state key semantics
- canonical value encoding
- tree node hashing
- proof verification
- state root computation

Changing the storage backend must not change block validity.

## Rejected Options

### Plain Key-Value Hash Of Database Records

Rejected because database iteration order, compaction behavior, and local schema
choices would create consensus risk.

### Unversioned Merkle Root

Rejected because HNChain must support long-term evolution of tree formats and
hash profiles.

### State Root Computed From JSON

Rejected because JSON is not a canonical consensus representation.

### Verkle Tree As Immediate Unconditional Requirement

Rejected for the first profile decision because Verkle trees reduce proof sizes
but introduce additional cryptographic assumptions, more complex implementation,
and a larger audit surface.

Verkle trees remain a future candidate if the cryptographic profile, proof
format, and implementation maturity justify the trade-off.

### Storage Backend As Consensus Format

Rejected because it would make protocol validity depend on a replaceable
implementation detail.

## Alternatives Considered

### Merkle Patricia Trie

Advantages:

- widely studied in account-based networks
- supports sparse keys
- has known operational experience

Disadvantages:

- complex encoding rules
- larger implementation and audit surface
- proof sizes may be larger than newer alternatives

### Sparse Merkle Tree

Advantages:

- simple fixed-depth proof semantics
- clean non-inclusion proofs
- good fit for deterministic hashed keys
- easier independent implementation

Disadvantages:

- proof size can be large without compression
- requires careful optimization for storage and caching
- naive implementation can be inefficient

### Jellyfish-Style Sparse Merkle Tree

Advantages:

- production-oriented sparse tree design
- supports versioned state efficiently
- good fit for authenticated account state

Disadvantages:

- still requires careful formal specification
- storage layout and proof format must be adapted, not copied blindly

### Verkle Tree

Advantages:

- smaller proofs
- promising for stateless and light-client designs

Disadvantages:

- more complex cryptography
- harder implementation and audit
- future post-quantum implications require separate analysis

## Security Considerations

State root divergence:

- Risk: honest nodes compute different roots for the same block.
- Mitigation: canonical keys, HNCS values, deterministic update order, explicit
  node hashing, and test vectors.

Proof ambiguity:

- Risk: multiple encodings verify against the same logical claim.
- Mitigation: canonical proof encoding, domain-separated node hashes, and strict
  parser rules.

Domain confusion:

- Risk: the same key material is valid in multiple state domains.
- Mitigation: domain-separated state key derivation.

State bloat:

- Risk: attackers create many long-lived records that increase validator
  storage and sync cost.
- Mitigation: versioned records, account lifecycle, extension loading,
  storage accounting, and future rent-policy support.

Snapshot poisoning:

- Risk: a node accepts an unauthenticated snapshot with false state.
- Mitigation: snapshot manifests must be verified against consensus-approved
  roots and checkpoints.

Pruning errors:

- Risk: nodes discard data required for validation, proofs, or rollback windows.
- Mitigation: explicit pruning profiles and capability declarations.

Hash profile migration:

- Risk: changing hash algorithms breaks historical proof verification.
- Mitigation: roots, proofs, and nodes include versioned tree and hash profiles.

## Compatibility

Adding a new state domain can be backward-compatible only if:

- the domain identifier is registered
- key derivation is specified
- value schemas are versioned
- activation rules are defined
- old nodes reject unsupported state transitions deterministically

Changing the tree profile or hash profile used for consensus state roots is a
major protocol change and requires explicit migration rules.

## Open Decisions

- initial tree profile
- state key length
- domain identifier registry
- state key derivation function
- empty state root value
- composite roots versus a single global tree
- proof format
- proof size limits
- pruning profiles
- archival node requirements
- snapshot manifest format
- update batching rules
- rollback window
- cache invalidation rules
- Verkle migration criteria
- state rent activation model

## Related Specifications

- `docs/specs/core/state-tree.md`
