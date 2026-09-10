# ADR-0022: Protocol Versioning

Status: Accepted

Date: 2026-09-10

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants

Supersedes: None

Referenced By:

- ADR-0003: Address Format
- ADR-0008: Block Format

## Context

ADR-0000 requires explicit versioning for every protocol object, network
packet, storage record, and public API, but does not say how the growing set
of independent version fields across the protocol relate to each other, or
how the network signals a hard fork.

Accepted and pending ADRs already define several version fields, not all of
which are independent of each other (see Decision):

- `hash_profile_id` (ADR-0005) versions the hash algorithm and domain
  separation construction.
- `tree_profile` (ADR-0007) versions the state tree's structural profile
  (`hn-smt-256-v1`).
- `state_key_version` (ADR-0007, inside `StateKeyInputV1` and
  `StateKeyInputExtensionV1`) versions the state key derivation byte layout.
- `envelope_version` (`docs/specs/core/account-state.md` §4.1) versions one
  account's envelope and section layout.
- `address_version` (ADR-0003, Proposed) will version `AddressPayload`.

Every future object-defining ADR (transaction format, block format, and so
on) will add its own version field the same way. Without a stated policy,
a later ADR could instead invent a single incrementing `protocol_version`
ad hoc, coupling unrelated structures together for no technical reason.

Separately, the network needs some way to coordinate a hard fork: to say
"at this height, this set of profiles and parameters is active." That is a
different problem from versioning an individual structure, and conflating
the two would make both harder to reason about.

## Decision

HNChain versions protocol objects along independent axes. There is no single
monolithic `protocol_version` that every structure shares.

Two distinct kinds of version exist and must not be conflated:

```text
Structure Version   - one per protocol object/structure, owned and bumped
                       only by the ADR or specification that defines that
                       structure.

Protocol Epoch       - one per network, recorded at the block level, naming
                       the set of ADR-defined profiles and parameters active
                       at a given height. Used only for hard-fork
                       coordination.
```

### Structure Versions

Every consensus structure keeps its own version field, changed only when
that structure's own encoding or semantics change. Current instances:

- `hash_profile_id` (ADR-0005)
- `tree_profile` (ADR-0007)
- `envelope_version` (account-state.md §4.1)
- `address_version` (ADR-0003, once accepted)

Changing one structure's version must not require bumping any other
structure's version. A future migration of `tree_profile` (for example, to a
Verkle-based profile) does not by itself require bumping `envelope_version`,
`hash_profile_id`, or `address_version`, and vice versa. If a change to one
structure genuinely requires a coordinated change to another, that
dependency must be stated explicitly by the owning ADR, not inferred from
parallel version numbers moving together.

Every future ADR that defines a new consensus structure (transaction format,
block format, validator set records, and so on) must give that structure its
own independent version field, per ADR-0000's Explicit Versioning invariant.
It must not reuse another structure's version field to imply its own
version, and must not omit a version field on the assumption that a global
counter covers it.

### Nested Structure Versions

Not every version field is a top-level independent axis. A structure
version may instead be scoped within, and owned by, another structure's
version — in which case it is not free to move independently of its parent.

`state_key_version` (ADR-0007, inside `StateKeyInputV1` and
`StateKeyInputExtensionV1`) is the current example: ADR-0007 defines it as
part of tree profile `0x0001`'s own specification, and its own Compatibility
section treats a change to state key derivation as a breaking change "for
tree profile `0x0001`" specifically, not as a globally independent field.
Concretely:

- `state_key_version` can change on its own while `tree_profile` stays
  `0x0001` (for example, refining the key derivation formula without
  changing the tree's structural model).
- `tree_profile` changing to a structurally different profile (for example,
  a Verkle-based profile, where a flat 256-bit hashed path may not even
  apply) should be assumed to require a new state-key derivation scheme as
  well, whether that takes the form of a new `state_key_version` or a
  different mechanism entirely defined by that profile.

The owning ADR must state which of its version fields are nested this way;
absent such a statement, a version field is treated as a top-level
independent axis under the rule above.

### Protocol Epoch

`protocol_epoch` is a monotonically increasing, network-scoped identifier,
distinct from every structure version above. It:

- is recorded at the block level; the exact field, width, and encoding are
  owned by ADR-0008 (Block Format), not this ADR.
- identifies the set of structure-version profiles accepted for new
  consensus objects at a given height (for example: "at `protocol_epoch` N,
  only `hash_profile_id = 0x0001` and `tree_profile = 0x0001` are valid for
  new state commitments").
- is the only signal nodes use to decide fork-choice and activation behavior
  around a hard fork. An individual structure-version bump is not, by
  itself, an activation signal.
- changes only through an explicit governance/activation process, owned by
  a future governance ADR, not this one.

## Normative Rules

### No Monolithic Version

No protocol object, ADR, or specification may introduce a single version
field intended to represent "the protocol version" for multiple independent
structures at once. Each structure versions itself.

### Version Independence

A structure's version field must change only when that structure's own
canonical encoding or semantics change. It must not be bumped to track
unrelated changes elsewhere in the protocol.

### Nested Versions Must Be Declared

A version field is nested within another structure's version only if the
owning ADR's own text establishes that scoping (for example, by treating a
change to the field as a breaking change "for" a specific parent profile
value, as ADR-0007 does for `state_key_version` under `tree_profile`).
Absent such text, a version field is independent under Version Independence.

This ADR classifying an already-Accepted field as nested (as it does for
`state_key_version` above) does not require rewriting the owning ADR to use
this document's vocabulary; the classification is drawn from that ADR's
existing normative text. Every ADR written after this one, however, must
state any nesting relationship explicitly, using this vocabulary, at the
point the version field is introduced — not add it later once a dependent
change is already needed.

### Protocol Epoch Is Explicit

`protocol_epoch` must be carried as an explicit field wherever fork-choice
or activation rules depend on it. Nodes must not infer the active profile
set from block height ranges, software version strings, or any other
implicit signal (ADR-0000, "No Hidden Consensus Dependencies").

### Structure Versions Are Not Activation Signals

Encountering a structure version a node does not recognize is a decode/
validation-time concern for that structure (accept, reject, or upgrade, per
ADR-0000). It must not, by itself, be treated as evidence that a hard fork
has occurred; that determination is `protocol_epoch`'s responsibility alone.

## Rejected Options

### Single Monolithic `protocol_version`

Advantages:

- one number to check
- simple mental model for a small protocol

Disadvantages:

- false coupling: a change to any one structure forces every implementation
  to reason about "what actually changed in this version" instead of
  checking that structure's own version field
- blocks independent evolution of unrelated structures (for example, a hash
  profile change would force an unrelated address format bump)
- larger review and audit surface per bump, since a single version number
  conflates unrelated decisions

Rejected because HNChain's structures (hash profiles, tree profiles, key
derivation, envelope layout, address format, and future transaction/block
formats) evolve for unrelated reasons and on unrelated cadences.

## Alternatives Considered

### Implicit Epoch From A Height-Activation Table

Advantages:

- smaller block header, since no epoch field is carried on-chain

Disadvantages:

- light clients and new nodes must fetch and trust an external
  height-to-profile activation table instead of reading self-contained
  header data
- harder to verify offline
- risk of independent implementations shipping mismatched activation tables,
  which is exactly the kind of hidden consensus dependency ADR-0000 forbids

Rejected as the primary mechanism; an activation table may still exist as
tooling metadata, but it is not the source of truth.

### No Epoch Concept, Hard Forks Coordinated Off-Chain

Advantages:

- nothing to specify now

Disadvantages:

- no in-band signal for validators or light clients to determine which rule
  set is active at a given height
- relies on off-chain software-version coordination, which ADR-0000 already
  rules out as a consensus dependency

Rejected for the same reason as the implicit activation table.

### Explicit `protocol_epoch` Field At The Block Level

Advantages:

- self-describing: the header alone reveals which profile set applies,
  without external tables
- gives fork-choice and light-client verification one canonical field to
  check

Disadvantages:

- adds a field to the block header (bounded, small cost)

Selected. Exact placement and encoding are deferred to ADR-0008.

## Security Considerations

Version confusion:

- Risk: a node infers a structure's version from context (digest length,
  field count) instead of an explicit field.
- Mitigation: ADR-0000's Explicit Versioning invariant, reaffirmed here for
  every structure listed above.

Fork ambiguity:

- Risk: nodes disagree about which profile set is active at a given height
  because there is no explicit epoch signal.
- Mitigation: mandatory `protocol_epoch` field, defined before any hard fork
  ships.

Downgrade confusion under a monolithic version:

- Risk: a single conflated version number makes it harder to tell which
  specific structure actually changed, which could be exploited to argue a
  stale sub-component's behavior is still "current."
- Mitigation: independent structure versions keep each structure's validity
  self-contained and separately checkable.

## Compatibility

Adding a new structure-version value (for example, a new `hash_profile_id`)
follows that structure's own owning ADR's compatibility rules; this ADR does
not add additional constraints beyond requiring the version field to exist.

Introducing `protocol_epoch` itself is a breaking change to block header
format if added after genesis. It should be present in the block header from
genesis (ADR-0008) rather than retrofitted.

Changing which axis (a structure version versus `protocol_epoch`) governs
fork-choice or activation decisions is a major protocol change.

## Deferred Decisions

- ~~exact `protocol_epoch` field width and encoding (owned by ADR-0008)~~
  — decided: `u64`, its own `BlockHeader` field distinct from the
  consensus-protocol `epoch` field (ADR-0008, "Protocol Epoch")
- governance and activation process for advancing `protocol_epoch` (owned by
  a future governance ADR)
- whether devnet and testnet carry `protocol_epoch` from their first genesis
  or only before mainnet
- format and location of a profile-compatibility matrix (which structure
  versions are valid at which `protocol_epoch`), if one is needed beyond
  each structure's own ADR

## Related Specifications

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0003-address-format.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/specs/core/account-state.md`
