# HNChain Core Specification: Hash Algorithms

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## 1. Scope

This document specifies the conceptual hash profile system for HNChain.

It defines hash profile structure, domain separation, lifecycle, allowed uses,
module boundaries, and security requirements.

This document does not finalize the primary hash algorithm, state tree layout,
transaction schema, block schema, proof format, or address derivation function.

This specification is constrained by:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`

## 2. Design Goals

- Hash only canonical HNCS bytes.
- Prevent cross-domain digest reuse.
- Support hash algorithm migration.
- Avoid algorithm inference from digest length.
- Make state tree hashing unambiguous.
- Require test vectors for every active profile.

## 3. Hash Profile

Conceptual structure:

```text
HashProfile
  profile_version
  hash_algorithm_id
  digest_length
  domain_tag
  allowed_uses
  lifecycle
```

Rules:

- `profile_version` identifies the profile format.
- `hash_algorithm_id` identifies the underlying hash algorithm.
- `digest_length` is explicit.
- `domain_tag` separates protocol domains.
- `allowed_uses` defines where the profile may be used.
- `lifecycle` defines activation status.

## 4. Hash Input

Every consensus hash input has this conceptual form:

```text
HashInput
  hash_profile_id
  domain_tag
  object_type
  object_version
  canonical_payload
```

`canonical_payload` is the HNCS encoding of the object being committed.

Exact encoding is defined by the HNCS schema for each object.

## 5. Domain Separation

Initial conceptual domain tags:

```text
hnchain.address.account.v1
hnchain.address.contract.v1
hnchain.transaction.id.v1
hnchain.transaction.signing.v1
hnchain.receipt.v1
hnchain.state.leaf.v1
hnchain.state.node.v1
hnchain.block.header.v1
hnchain.block.id.v1
hnchain.p2p.message.v1
hnchain.registry.algorithm.v1
```

Rules:

- domain tags are protocol constants
- domain tags are included in hash input construction
- domain tags are not user-provided strings
- domain tags are versioned
- a digest from one domain must not be accepted in another domain

## 6. Allowed Uses

Initial allowed-use categories:

```text
address_derivation
transaction_id
transaction_signing
receipt_commitment
state_leaf
state_internal_node
state_root
block_header
block_id
p2p_message_id
registry_commitment
```

A hash profile may be valid for one or more allowed-use categories.

Using a profile outside its allowed uses is invalid.

## 7. Algorithm Lifecycle

Hash profile lifecycle:

```text
Proposed -> Active -> Deprecated -> Disabled
```

Rules:

- `Proposed` profiles are not valid for consensus commitments.
- `Active` profiles are valid for configured uses.
- `Deprecated` profiles may verify historical or migration data according to
  protocol rules.
- `Disabled` profiles are rejected except for explicitly specified historical
  verification.

## 8. Tree Hashing Requirements

Any state tree or Merkle-like structure must define separate hash domains for:

- empty nodes
- leaf nodes
- internal nodes
- extension or compressed path nodes, if used
- proof nodes

Tree hash inputs must be length-delimited and canonical.

Ad hoc concatenation is forbidden.

## 9. Module Boundaries

```text
HNCS Encoder
     |
     v
Hash Profile Resolver
     |
     v
Domain-Separated Hash
     |
     +----> Address Derivation
     +----> Transaction ID
     +----> State Root
     +----> Block ID
     +----> Proof Verification
```

Boundary rules:

- Serialization defines canonical bytes.
- Hash profiles define digest construction.
- Address format defines how address bodies use hash outputs.
- State tree defines node layout and which profiles are used.
- Transaction and block specs define object-specific hash contexts.

## 10. Security Requirements

- Bare hash calls are forbidden in consensus code.
- Hash algorithms are never inferred from digest length.
- Domain tags must be included in test vectors.
- Hash input construction must be length-delimited.
- Profile lifecycle must be enforced by validation.
- Deprecated profiles must not be accepted for new objects unless migration rules
  explicitly allow it.
- Wallet password hashing must use a separate wallet security specification.

## 11. Open Architecture Decisions

- final primary consensus hash profile
- final digest length per domain
- final domain tag binary encoding
- final algorithm identifier registry
- address derivation profile
- state tree hash profiles
- transaction ID hash profile
- block ID hash profile
- test vector schema
