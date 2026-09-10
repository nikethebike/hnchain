# ADR-0007: State Tree

Status: Accepted

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms

Supersedes: None

Referenced By:

- ADR-0006: Transaction Format
- ADR-0008: Block Format
- ADR-0019: Storage And State Interfaces

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

HNChain accepts the initial state tree profile:

```text
State Version:       1
Tree Profile ID:     0x0001
Tree Profile Name:   hn-smt-256-v1
Tree Type:           binary sparse Merkle tree
Tree Depth:          256 bits
State Key Length:    32 bytes
Hash Profile ID:     0x0001
Hash Profile Name:   hn-sha512-256-v1
Compressed Paths:    not supported in v1
Root Model:          single global state root
```

The profile is intentionally conservative. A 256-bit sparse Merkle tree has
simple inclusion and non-inclusion semantics, fixed-length keys, a clear
light-client proof model, and avoids introducing Verkle-specific cryptographic
assumptions in the initial protocol.

Jellyfish-style storage and batching techniques may be used as implementation
inspiration, but the consensus profile is HNChain-specific and is defined by
this ADR plus the state tree specification, not by an external database layout.

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

Key identity versus value content:

The state key encodes object identity only — which domain, which section or
extension, which object, which subkey. It never encodes schema version,
criticality, or field content. Schema version and value content live
exclusively inside the committed value's canonical HNCS payload (see Canonical
State Values). Two accounts on different schema versions of the same section
still derive the same state key; version divergence is resolved by the value
decoder, not by key routing.

Initial state key derivation defines two input schemas. A core schema
addresses domain sections by a section identifier. An extension schema
addresses extension records by an extension identifier. The schema used for a
given key is selected by `domain_id`.

Core section state key:

```text
state_key = HASH_PROFILE_0x0001(
  domain = "hnchain.state.key.v1",
  payload = HNCS(StateKeyInputV1)
)

StateKeyInputV1
  u16   state_key_version = 1
  u8    domain_id
  u8    section_id
  bytes object_id
  bytes subkey
```

Canonical byte layout (integer and byte-sequence framing per ADR-0004):

```text
[0..2)          state_key_version   u16, little-endian, = 1
[2..3)          domain_id           u8
[3..4)          section_id          u8
[4..8)          object_id_len       u32, little-endian
[8..8+N)        object_id           N = object_id_len raw canonical bytes
[8+N..12+N)     subkey_len          u32, little-endian
[12+N..12+N+M)  subkey              M = subkey_len raw canonical bytes
```

Extension state key:

```text
state_key = HASH_PROFILE_0x0001(
  domain = "hnchain.state.key.v1",
  payload = HNCS(StateKeyInputExtensionV1)
)

StateKeyInputExtensionV1
  u16   state_key_version = 1
  u8    domain_id
  u16   extension_id
  bytes object_id
  bytes subkey
```

Canonical byte layout:

```text
[0..2)          state_key_version   u16, little-endian, = 1
[2..3)          domain_id           u8
[3..5)          extension_id        u16, little-endian
[5..9)          object_id_len       u32, little-endian
[9..9+N)        object_id           N = object_id_len raw canonical bytes
[9+N..13+N)     subkey_len          u32, little-endian
[13+N..13+N+M)  subkey              M = subkey_len raw canonical bytes
```

Rules:

- `domain_id` is `uint8`. Values are assigned by the State Domains registry
  below.
- `section_id` is `uint8`. It is meaningful only for domains that define a
  section registry (initially `accounts`; see Accounts Domain: Sections).
- `extension_id` is `uint16`. `uint16` is chosen over `uint32` because
  extension identifiers are assigned by a curated protocol registry rather
  than derived from unbounded third-party input, and 65536 slots exceed any
  realistic near-term registry size; a future state key version may widen
  this field if the registry approaches exhaustion.
- `object_id` is the raw canonical bytes of the domain's identifying object,
  used as-is and never re-wrapped in an additional HNCS structure. For
  address-keyed domains, `object_id` is the canonical protocol address bytes
  (ADR-0003), treated opaquely without reinterpreting their internal fields.
- `subkey` is a domain-specific canonical subkey, bounded by the domain
  schema. It is empty (`subkey_len = 0`) for every leaf class defined by this
  ADR.
- `object_id` and `subkey` use the canonical `u32_length || bytes` framing
  defined by ADR-0004.
- the output of `state_key` is exactly 32 bytes.
- collisions are consensus-fatal and are treated as cryptographic hash
  collisions, not as overwrite behavior.

### State Domains

The state tree must support independent domains.

Initial conceptual domains:

- `0x0001` accounts
- `0x0002` account_extensions
- `0x0003` contracts
- `0x0004` contract_storage
- `0x0005` assets
- `0x0006` validators
- `0x0007` governance
- `0x0008` metadata
- `0x0009` system

Domains are part of the key derivation and proof semantics.

Domain ID `0x0000` is reserved and invalid for committed state.

The `accounts` and `account_extensions` domains define additional per-domain
leaf structure, specified below.

### Accounts Domain: Sections

The `accounts` domain (`0x0001`) is subdivided into sections by `section_id`.
Each account section occupies exactly one leaf. A section's value is never
split across multiple leaves; a section that grows is versioned in place, not
fragmented into additional state keys.

SectionId registry:

- `0x00` envelope
- `0x01` identity
- `0x02` balance
- `0x03` nonce
- `0x04` permission
- `0x05` metadata
- `0x06` asset
- `0x07` lifecycle

This registry is closed for tree profile `0x0001`. Adding a section requires a
new `section_id` value and is a compatibility change under Compatibility
below.

`extension state` (ADR-0001, `docs/specs/core/account-state.md` §4.8) is
deliberately excluded from this registry. It is addressed through the
`account_extensions` domain instead, because its cardinality and payload size
differ from the fixed core sections above.

The envelope leaf (`section_id = 0x00`) is a leaf like any other. Its value is
the envelope object defined in `docs/specs/core/account-state.md` §4.1
(`envelope_version`, `account_type`, `address`, `section_versions`).
`section_versions` records the version of every populated section for the
account, so a reader can determine which section versions apply without
probing every section leaf. This ADR defines only where the envelope lives in
the tree; the internal encoding of `section_versions` belongs to the account
state specification.

### Account Extensions Domain: Registry And Payload Leaves

The `account_extensions` domain (`0x0002`) holds two leaf classes per account,
distinguished by `extension_id`:

- `extension_id = 0x0000` (reserved): the extension **registry** leaf. Its
  value lists the extensions installed for the account (extension identifier,
  version, criticality flag) as defined in
  `docs/specs/core/account-state.md` §4.8.
- `extension_id = 0x0001` .. `0xFFFF`: an extension **payload** leaf. Each
  value holds the canonical payload for exactly one installed extension.

Loading the registry leaf does not require loading any payload leaf, matching
the lazy extension loading rule required by ADR-0001 and
`docs/specs/core/account-state.md` §4.8.

Assignment of concrete `extension_id` values to extension types, and the
activation process for new extensions, are extension registry mechanics and
are out of scope for this ADR. They are tracked as an open architecture
decision in `docs/specs/core/account-state.md` §10.

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

Accepted node classes for `hn-smt-256-v1`:

- empty node
- leaf node
- internal node

Each node type must have a domain-separated hash encoding.

Compressed path nodes are not supported in the initial profile. Encodings that
attempt to introduce compressed path nodes under tree profile `0x0001` are
invalid.

Conceptual node hash inputs:

```text
EmptyNodeV1
  u16 tree_profile = 0x0001

LeafNodeV1
  u16 tree_profile = 0x0001
  bytes32 state_key
  bytes32 value_hash

InternalNodeV1
  u16 tree_profile = 0x0001
  bytes32 left_child_hash
  bytes32 right_child_hash
```

Domain tags:

```text
hnchain.state.empty.v1
hnchain.state.leaf.v1
hnchain.state.internal.v1
hnchain.state.value.v1
hnchain.state.key.v1
```

### Empty Root Construction

Empty subtree hashes are defined recursively:

```text
empty_hash[0] = HASH_PROFILE_0x0001(
  domain = "hnchain.state.empty.v1",
  payload = HNCS(EmptyNodeV1{ tree_profile = 0x0001 })
)

empty_hash[d] = HASH_PROFILE_0x0001(
  domain = "hnchain.state.internal.v1",
  payload = HNCS(InternalNodeV1{
    tree_profile     = 0x0001,
    left_child_hash  = empty_hash[d-1],
    right_child_hash = empty_hash[d-1],
  })
)  for d = 1 .. 256

empty_root = empty_hash[256]
```

`empty_hash[d]` is the canonical hash of a fully empty subtree of depth `d`.
The empty state root, used when no state keys are committed, is
`empty_root = empty_hash[256]`, matching `Tree Depth: 256 bits` in the profile
header. Implementations may precompute and cache `empty_hash[0..256]`, since
these values depend only on the tree and hash profile, not on committed
state.

### Leaf Preimage

```text
LeafHash = HASH_PROFILE_0x0001(
  domain = "hnchain.state.leaf.v1",
  payload = HNCS(LeafNodeV1{
    tree_profile = 0x0001,
    state_key,
    value_hash,
  })
)
```

`state_key` is produced by the core or extension state key derivation defined
in Canonical State Keys. `value_hash` is produced by the value hash formula
below.

The value hash is:

```text
value_hash = HASH_PROFILE_0x0001(
  domain = "hnchain.state.value.v1",
  payload = HNCS(StateValueV1)
)
```

### Internal Node Child Order

For a committed `state_key` (32 bytes, 256 bits), depth `d` (`d = 0` at the
root, `d = 255` at the last internal level before leaves) routes on bit `d` of
`state_key`, read most-significant-bit-first within each byte (bit `0` is the
most significant bit of byte `0`).

- bit value `0` routes to `left_child_hash`
- bit value `1` routes to `right_child_hash`

`InternalNodeV1.left_child_hash` is always the subtree reached by routing bit
`0`, and `right_child_hash` is always the subtree reached by routing bit `1`,
regardless of which numeric hash value is larger. The preimage order is always
`left_child_hash || right_child_hash`; implementations must never reorder
children by comparing hash values.

### Updates

Block execution updates the state tree by applying validated transaction effects
in deterministic block order.

Parallel execution is allowed only if the final commit order is deterministic
and produces the same root as the canonical serial execution rule.

The canonical state tree update order is bytewise ascending order of 32-byte
state keys after all transaction effects for the block have been validated and
merged according to execution rules.

If multiple state transitions in the same block write the same state key, the
execution specification must define the final value before the write set reaches
the state tree. The state tree layer accepts a deterministic final write set; it
does not resolve transaction conflicts.

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

### Single Global Root

HNChain uses a single global authenticated state root in the initial profile.

State domains are separated through state key derivation rather than separate
per-domain roots.

Rationale:

- block headers commit to one state root
- light-client verification has a single root target
- domain additions do not require changing the block header root structure
- storage backends remain free to shard or index data internally

Trade-off:

- per-domain root extraction requires either proofs or derived indexes
- future parallelism must be achieved through deterministic write sets and
  storage engineering rather than independent consensus roots

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

Selected as the initial consensus tree profile with a fixed 256-bit keyspace.

### Jellyfish-Style Sparse Merkle Tree

Advantages:

- production-oriented sparse tree design
- supports versioned state efficiently
- good fit for authenticated account state

Disadvantages:

- still requires careful formal specification
- storage layout and proof format must be adapted, not copied blindly

Not selected as an external protocol dependency. Its implementation ideas may
inform storage design, but HNChain defines its own `hn-smt-256-v1` consensus
profile.

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

Changing state key derivation, state domain identifiers, section identifiers,
extension identifier reservation (`0x0000`), node hash inputs, internal node
child ordering, empty root computation, or update ordering for tree profile
`0x0001` is a breaking protocol change.

## Deferred Decisions

- exact proof wire format
- proof size limits
- pruning profiles
- archival node requirements
- snapshot manifest format
- rollback window
- cache invalidation strategy
- Verkle migration criteria
- state rent activation model

## Related Specifications

- `docs/specs/core/state-tree.md`
- `docs/specs/core/block-format.md`
- `docs/rfc/storage/storage-state-interfaces.md`
