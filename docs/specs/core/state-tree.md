# HNChain Core Specification: State Tree

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0001-account-state-model.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0006-transaction-format.md`

## 1. Purpose

This specification defines the HNChain authenticated state tree model.

The state tree provides the cryptographic commitment known as `state_root`.
`state_root` is included in block headers and represents the complete consensus
state after block execution.

## 2. Design Boundary

The state tree is a protocol object.

The storage engine is an implementation detail.

```text
Block Execution
  -> State Transition
  -> Canonical State Update
  -> Authenticated State Tree
  -> State Root
  -> Block Header
```

RocksDB, another embedded database, or a custom storage layer may store state
tree nodes. None of them define consensus semantics.

## 3. State Commitment

Every block commits to state through a versioned state commitment.

Conceptual structure:

```text
StateCommitmentV1
  state_version
  tree_profile
  hash_profile
  block_height
  state_root
```

### 3.1 Fields

`state_version`

- Protocol version of the state commitment structure.
- Initial value: open.

`tree_profile`

- Identifier of the active authenticated tree rules.
- Initial value: open.

`hash_profile`

- Identifier of the hash algorithm and domain separation rules used by the tree.
- Initial value: defined by the accepted hash algorithm specification.

`block_height`

- Height after applying the block that produced this commitment.

`state_root`

- Root commitment over canonical state tree nodes.

## 4. State Domains

State is divided into protocol domains.

Initial domain registry:

```text
accounts
account_extensions
contracts
contract_storage
assets
validators
governance
metadata
system
```

Domain identifiers are consensus values. Display names are documentation labels.

Adding a domain requires a protocol change record and activation rule.

## 5. State Keys

Every committed value is addressed by a canonical state key.

Conceptual function:

```text
STATE_KEY(domain, object_id, subkey) -> bytes
```

The final function must be domain-separated and deterministic.

It must define:

- input encodings
- output length
- hash profile
- reserved domains
- invalid inputs
- test vectors

State keys must not use RPC strings, JSON objects, local database keys, or
filesystem paths as consensus input.

## 6. State Values

Every committed state value is an HNCS value.

Conceptual structure:

```text
StateValueV1
  value_version
  value_type
  payload
```

`payload` is the canonical HNCS encoding of the domain-specific object.

Examples:

- account core record
- account extension record
- contract code record
- contract storage cell
- asset definition
- validator status record
- governance proposal record

## 7. Tree Nodes

The exact node structure depends on `tree_profile`.

Every accepted tree profile must define:

- node types
- canonical node encoding
- child ordering
- empty node representation
- leaf representation
- internal node representation
- compressed path representation, if supported
- node hash domains
- proof verification rules

Conceptual node classes:

```text
EmptyNode
LeafNode
InternalNode
CompressedPathNode
```

If a tree profile does not support compressed paths, it must explicitly reject
`CompressedPathNode`.

## 8. Root Computation

Root computation is deterministic.

For a given:

- previous state root
- ordered transaction list
- state transition rules
- tree profile
- hash profile

all honest nodes must produce exactly one next state root.

The root must not depend on:

- thread scheduling
- CPU architecture
- database iteration order
- cache state
- node startup order
- RPC request order
- local wall-clock time

## 9. Block Execution Interface

The execution layer produces a deterministic state write set.

Conceptual flow:

```text
previous_state_root
  -> ordered_transactions
  -> validation
  -> execution
  -> deterministic_write_set
  -> state_tree_update
  -> next_state_root
```

Parallel execution is valid only if its final write set and commit order are
equivalent to the canonical serial execution rule.

## 10. Proofs

The state tree must support:

- inclusion proofs
- non-inclusion proofs

Conceptual proof structure:

```text
StateProofV1
  proof_version
  tree_profile
  hash_profile
  state_root
  query_key
  proof_nodes
```

Proof verification must be possible without access to a full node database.

Proof consumers include:

- light clients
- wallets
- explorers
- bridges
- audit tools
- snapshot verifiers

## 11. Snapshots

A snapshot represents state data for a known state root.

Snapshots must include or reference:

- snapshot version
- block height
- block hash
- state root
- tree profile
- hash profile
- chunk format
- manifest hash
- producer metadata
- verification metadata

Snapshot acceptance requires verification against consensus-approved block or
checkpoint data.

## 12. Pruning Profiles

Nodes may implement different local pruning policies.

Conceptual profiles:

```text
archive
full_recent
validator_recent
light
```

The profile determines local retention behavior. It does not change block
validity.

Validator requirements for state retention are consensus and operations policy
decisions and must be specified separately.

## 13. State Bloat Controls

The state tree must remain compatible with future state-bloat controls.

Required design hooks:

- versioned state values
- account lifecycle states
- lazy account extensions
- explicit storage accounting
- future rent-policy activation
- archival and pruning profiles
- snapshot verification

Rent policy is not activated by this specification.

## 14. Tree Profile Selection Criteria

The initial tree profile must be selected using documented evidence.

Evaluation criteria:

- deterministic specification simplicity
- proof size
- update cost
- read performance
- write amplification
- implementation complexity
- auditability
- compatibility with snapshots
- compatibility with light clients
- compatibility with future hash migration
- long-term maintenance cost

Headline TPS claims must not drive tree selection without benchmark conditions
and reproducible tests.

## 15. Security Requirements

Implementations must reject:

- unknown tree profiles
- unknown hash profiles
- malformed node encodings
- duplicate proof paths
- non-canonical HNCS values
- oversized proofs
- inconsistent domain identifiers
- state keys with invalid length
- state values with unsupported schema versions

Implementations must bound:

- proof verification time
- proof memory usage
- update batch size
- snapshot chunk size
- tree node decode cost

## 16. Test Vectors

The accepted version of this specification must include test vectors for:

- empty state root
- single account insertion
- account update
- account deletion or lifecycle transition
- extension insertion
- multiple domain updates
- inclusion proof
- non-inclusion proof
- malformed proof rejection
- deterministic batch ordering

Test vectors are mandatory before production implementation.

## 17. Open Decisions

- initial `state_version`
- initial `tree_profile`
- initial state key length
- final domain registry numeric identifiers
- composite root or single tree root
- empty state root
- proof wire format
- snapshot manifest format
- pruning window
- rollback window
- storage backend layout recommendations
- state rent economics

