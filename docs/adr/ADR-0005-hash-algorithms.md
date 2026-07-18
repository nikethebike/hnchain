# ADR-0005: Hash Algorithms

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0003: Address Format
- ADR-0004: Canonical Serialization

Supersedes: None

## Context

HNChain requires hash functions for address derivation, transaction identifiers,
block identifiers, state roots, receipts, storage commitments, proof systems,
P2P message identifiers, and protocol registries.

Hashing must be specified after canonical serialization because protocol hashes
must be computed over canonical bytes.

The protocol must define not only hash algorithms, but also domain separation,
hash profile identifiers, digest lengths, upgrade rules, and object-specific
hashing contexts.

## Decision

HNChain uses versioned hash profiles.

A hash profile defines:

- algorithm identifier
- digest length
- domain separation method
- input construction
- output truncation rules, if any
- valid protocol uses
- lifecycle state
- test vectors

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

Protocol code must not call a bare hash function for consensus data.

Every consensus hash must be computed through a named hash profile over
canonical HNCS bytes.

## Normative Rules

### Hash Only Canonical Bytes

Consensus hashes are computed only over canonical HNCS encodings.

Hashing JSON, display strings, RPC objects, in-memory structs, database rows, or
transport-specific messages is forbidden for consensus commitments.

### Mandatory Domain Separation

Every protocol hash must use explicit domain separation.

Initial conceptual domain tags:

- `hnchain.address.account.v1`
- `hnchain.address.contract.v1`
- `hnchain.transaction.id.v1`
- `hnchain.transaction.signing.v1`
- `hnchain.receipt.v1`
- `hnchain.state.leaf.v1`
- `hnchain.state.node.v1`
- `hnchain.block.header.v1`
- `hnchain.block.id.v1`
- `hnchain.p2p.message.v1`
- `hnchain.registry.algorithm.v1`

Domain tags are protocol constants. They are not user input.

### No Cross-Domain Hash Reuse

A digest produced for one domain must not be accepted as a digest for another
domain unless the specification explicitly defines such equivalence.

### Hash Profile Identifier

Every protocol object that commits to a hash algorithm must define the hash
profile used.

Nodes must not infer hash algorithms from digest length.

### Digest Length

Digest length is part of the hash profile.

Truncation is forbidden unless the profile explicitly defines truncation,
security rationale, and collision risk.

### Algorithm Lifecycle

Hash algorithms and profiles follow lifecycle states:

```text
Proposed -> Active -> Deprecated -> Disabled
```

Changing the hash profile for state roots, transaction identifiers, or block
identifiers is a major protocol change unless the object specification already
defines a versioned migration path.

### Merkle and State Tree Hashing

Tree hashing must distinguish leaf nodes, internal nodes, empty nodes, extension
nodes, and any future node class through domain separation.

Tree hash input must not be ambiguous under concatenation.

### Password Hashing Exclusion

Consensus hash profiles are not password hashing profiles.

Wallet password storage, key encryption, and user secret handling require a
separate wallet security specification.

## Initial Algorithm Candidates

This ADR does not yet accept a final primary hash suite.

### SHA-256

Advantages:

- widely deployed
- mature implementations
- strong hardware and library support
- standardized by NIST FIPS 180-4

Disadvantages:

- less flexible for XOF-style output than SHAKE
- vulnerable to length-extension if used incorrectly outside a safe construction
- not the fastest option on all modern platforms

Recommended role if selected:

- compatibility hashing
- conservative transaction and block identifiers

### SHA-512/256

Advantages:

- 256-bit digest with SHA-512-family internal structure
- good performance on 64-bit CPUs
- standardized by NIST FIPS 180-4

Disadvantages:

- less common in blockchain tooling than SHA-256
- still requires clear domain separation

Recommended role if selected:

- primary conservative 256-bit digest candidate

### SHA3-256

Advantages:

- standardized by NIST FIPS 202
- sponge construction avoids SHA-2 length-extension class issues
- conservative standardization profile

Disadvantages:

- may be slower than SHA-2 or BLAKE3 in common software implementations
- NIST has announced an update process for FIPS 202, which should be tracked
  before final lock-in

Recommended role if selected:

- conservative state commitment or registry hashing

### SHAKE256

Advantages:

- extendable-output function
- useful when different digest lengths are required under one construction
- standardized by NIST FIPS 202

Disadvantages:

- output length must be specified carefully
- less familiar for wallet and explorer ecosystems

Recommended role if selected:

- proof systems, commitments requiring configurable output length

### BLAKE3

Advantages:

- high software performance
- parallelizable design
- fixed 32-byte default digest
- official specifications and implementations exist

Disadvantages:

- not a NIST FIPS standard
- younger ecosystem than SHA-2
- performance-oriented design does not remove the need for conservative review
- not suitable for password hashing

Recommended role if selected:

- non-consensus content addressing or high-throughput internal commitments after
  review
- possible future consensus profile only after formal acceptance

## Recommended Direction

HNChain should use hash profile agility from genesis.

The recommended initial direction is:

- SHA-512/256 or SHA3-256 as the conservative primary consensus digest
  candidate.
- SHA-256 retained as a compatibility profile where ecosystem integration
  requires it.
- SHAKE256 reserved for future proof systems or variable-output commitments.
- BLAKE3 considered for non-consensus performance-sensitive content addressing
  first, and only later for consensus if benchmarks, audits, and governance
  accept the trade-off.

This ADR does not finalize the primary hash suite. Final acceptance requires
benchmarking, implementation review, test vectors, and state tree requirements.

## Rejected Practices

### Bare Hash Calls

Example:

```text
hash(bytes)
```

Rejected because it lacks domain separation and profile identity.

### Hashing JSON

Rejected because JSON object ordering, number representation, string escaping,
Unicode handling, and display formatting are unsuitable for consensus
commitments.

### Inferring Hash Algorithm From Digest Length

Rejected because multiple algorithms can produce the same digest length.

### Silent Digest Truncation

Rejected because truncation changes security properties and collision risk.

## Security Considerations

Consensus split:

- Risk: nodes hash different byte representations.
- Mitigation: hash only canonical HNCS bytes.

Cross-domain collision:

- Risk: one digest is reused across address, transaction, state, or block
  domains.
- Mitigation: mandatory domain separation.

Algorithm downgrade:

- Risk: attackers cause weaker or deprecated hash profiles to be accepted.
- Mitigation: explicit profile identifiers and lifecycle checks.

State tree ambiguity:

- Risk: leaf and internal nodes can collide by construction.
- Mitigation: node-type domain tags and length-delimited inputs.

Length-extension misuse:

- Risk: Merkle or signing constructions misuse Merkle-Damgard hash functions.
- Mitigation: domain-separated, length-delimited HNCS inputs and no ad hoc
  concatenation.

Hash migration:

- Risk: changing state root or block hash algorithms breaks historical
  verification.
- Mitigation: versioned object formats and explicit migration rules.

## Compatibility

Adding a new hash profile can be backward-compatible only if:

- the profile has a unique identifier
- allowed uses are defined
- domain tags are specified
- digest length is specified
- unsupported nodes reject it deterministically
- no existing object silently changes its hash interpretation

Changing the hash profile for an existing consensus object version is a major
protocol change.

## Related Specifications

- `docs/adr/ADR-0006-transaction-format.md`
- `docs/specs/core/hash-algorithms.md`
- `docs/specs/core/transaction-format.md`

## Open Decisions

- final primary consensus hash profile
- final hash profile identifier encoding
- final domain tag binary representation
- whether state tree and transaction identifiers use the same algorithm
- whether address derivation uses the same algorithm as transaction IDs
- digest length for each protocol domain
- BLAKE3 role, if any, in consensus
- hash test vector format
- approved cryptographic libraries

## References

- NIST FIPS 180-4: Secure Hash Standard
  https://csrc.nist.gov/pubs/fips/180-4/upd1/final
- NIST FIPS 202: SHA-3 Standard
  https://csrc.nist.gov/pubs/fips/202/final
- NIST SHA-3 update announcement
  https://www.nist.gov/news-events/news/2025/03/sha-3-nist-update-fips-202-and-revise-special-publication-800-185
- BLAKE3 specifications
  https://github.com/BLAKE3-team/BLAKE3-specs
