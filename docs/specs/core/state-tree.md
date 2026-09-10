# HNChain Core Specification: State Tree

Status: Accepted

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0001-account-state-model.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0019-storage-state-interfaces.md`

Referenced By:

- `tests/conformance/core/state-tree-v0.1.json`

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
- Initial value: `1` (ADR-0007).

`tree_profile`

- Identifier of the active authenticated tree rules.
- Initial value: `0x0001` (`hn-smt-256-v1`, ADR-0007). Per ADR-0022, this is
  a Structure Version and `state_key_version` (§5) is nested under it, not
  independent: a future `tree_profile` change should be assumed to require
  a new key derivation scheme too.

`hash_profile`

- Identifier of the hash algorithm and domain separation rules used by the tree.
- Initial value: `0x0001` (`hn-sha512-256-v1`, ADR-0005).

`block_height`

- Height after applying the block that produced this commitment.

`state_root`

- Root commitment over canonical state tree nodes.

## 4. State Domains

State is divided into protocol domains, each an explicit numeric consensus
identifier (ADR-0007). `0x0000` is reserved and invalid for committed state.

```text
0x0001  accounts
0x0002  account_extensions
0x0003  contracts
0x0004  contract_storage
0x0005  assets
0x0006  validators
0x0007  governance
0x0008  metadata
0x0009  system
```

Display names above are documentation labels; the numeric identifier is the
consensus value.

The `accounts` domain has a closed SectionId registry (envelope, identity,
balance, nonce, permission, metadata, asset, lifecycle), and
`account_extensions` splits into a registry leaf (`extension_id = 0x0000`)
and per-extension payload leaves (`0x0001..0xFFFF`); see ADR-0007's Accounts
Domain: Sections and Account Extensions Domain sections.

Adding a domain requires a protocol change record and activation rule.

## 5. State Keys

Every committed value is addressed by a canonical state key, resolved by
ADR-0007 into two input schemas: a core schema for domain sections
(addressed by `section_id`) and an extension schema for extension records
(addressed by `extension_id`).

```text
state_key = HASH_PROFILE_0x0001("hnchain.state.key.v1", HNCS(StateKeyInputV1 | StateKeyInputExtensionV1))
```

The output is exactly 32 bytes. `domain_id` and `section_id` are `uint8`;
`extension_id` is `uint16`; `object_id` and `subkey` are canonical bytes,
length-delimited per ADR-0004, bounded by the reference implementation to
64 bytes each (`hn-state::key::{OBJECT_ID_MAX_LEN, SUBKEY_MAX_LEN}` — an
implementation resource bound, not a consensus value; see ADR-0007's note on
revisiting it once ADR-0003 fixes a final address body length). The exact
byte layout for both schemas is defined in ADR-0007's Canonical State Keys
section, not repeated here.

The state key encodes object identity only — domain, section or extension,
object, subkey. It never encodes schema version or content; those live in
the committed value (§6).

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

The exact node structure depends on `tree_profile`. For `tree_profile =
0x0001` (`hn-smt-256-v1`), ADR-0007 defines exactly three node classes;
compressed path nodes are not supported and must be rejected if
encountered:

```text
EmptyNode
LeafNode
InternalNode
```

Each has a domain-separated hash (`hnchain.state.empty.v1`,
`hnchain.state.leaf.v1`, `hnchain.state.internal.v1`). Empty subtree hashes
are defined recursively from `empty_hash[0]` up to `empty_hash[256]`
(the empty state root); internal node preimages are always
`left_child_hash || right_child_hash`, where the child at bit `0` of the
routed `state_key` (read MSB-first) is left and bit `1` is right — never
reordered by comparing hash values. Exact formulas are in ADR-0007's Node
Types, Empty Root Construction, Leaf Preimage, and Internal Node Child
Order sections.

A future tree profile that supports compressed paths, or any other node
class, must define its own node types, encoding, and hash domains the same
way; it does not inherit `hn-smt-256-v1`'s three-node-class shape.

## 8. Root Computation

HNChain uses a single global state root (ADR-0007), not a composite root or
per-domain roots: state domains are separated through state key derivation
(§5), not separate root fields in the block header.

The canonical update order is bytewise ascending order of 32-byte state
keys, applied after all of a block's transaction effects are validated and
merged into a final write set; the state tree layer accepts that write set
as given and does not itself resolve write-set conflicts (ADR-0007,
Updates).

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

`tests/conformance/core/state-tree-v0.1.json`, with its runner in
`crates/hn-state/tests/state_tree_conformance.rs`, is this specification's
conformance suite for the `hn-smt-256-v1` tree math: empty-root recursion,
core and extension state key derivation (including an invalid oversized
`object_id`/`subkey` case), leaf and internal node hashing, state root
computation over single/two-account and extension registry-plus-payload
write sets, and update-order independence (including a rejected duplicate
`state_key` case). Every value in it is cross-checked against an
independent oracle, not only self-consistent with the reference
implementation.

Still missing from that vector set, tracked as its own `promote_to_stable_when`
condition rather than duplicated here: `object_id`/`subkey` vectors using a
real (not opaque-placeholder) canonical address once ADR-0003 is Accepted.

Proof vectors (inclusion, non-inclusion, malformed proof rejection) and
account lifecycle/deletion vectors are not yet covered: proof wire format
is an ADR-0007 Deferred Decision (§17), and account lifecycle transitions
depend on a value schema this specification does not define (§6). Both
remain required before production implementation, per this section's
original intent.

## 17. Open Decisions

Resolved by ADR-0007 and removed from this list: initial `state_version`
(`1`), initial `tree_profile` (`0x0001`, `hn-smt-256-v1`), initial state
key length (32 bytes), final domain registry numeric identifiers
(`0x0001`-`0x0009`), composite root or single tree root (single global
root), and empty state root (recursive `empty_hash[0..256]`).

Still open, reproduced verbatim from ADR-0007's own Deferred Decisions
(that ADR owns these; this list must track it exactly, not paraphrase it):

- exact proof wire format
- proof size limits
- pruning profiles
- archival node requirements
- snapshot manifest format
- rollback window
- cache invalidation strategy
- Verkle migration criteria
- state rent activation model

Still open, owned by this specification rather than ADR-0007 (ADR-0007
deliberately does not define storage backend behavior; see §2 and ADR-0007's
Storage Backend Independence):

- storage backend layout recommendations
