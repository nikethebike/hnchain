# ADR-0005: Hash Algorithms

Status: Accepted

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0004: Canonical Serialization

Supersedes: None

Referenced By:

- ADR-0003: Address Format

## Context

HNChain requires hash functions for address derivation, transaction identifiers,
block identifiers, state roots, receipts, storage commitments, proof systems,
P2P message identifiers, and protocol registries.

Hashing must be specified after canonical serialization because protocol hashes
must be computed over canonical bytes.

The protocol must define not only hash algorithms, but also domain separation,
hash profile identifiers, digest lengths, upgrade rules, and object-specific
hashing contexts.

Address derivation is a consumer of this hash profile, but the hash profile does
not depend on the final address text encoding, checksum scheme, or address
payload layout. ADR-0003 may wrap, encode, or further constrain address digests
without changing the accepted hash profile.

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

The initial HNChain v0.1 consensus hash profile is:

```text
Hash Profile ID:     0x0001
Profile Name:        hn-sha512-256-v1
Algorithm:           SHA-512/256
Digest Length:       32 bytes
Lifecycle:           Active
Specification:       NIST FIPS 180-4
Input Encoding:      HNCS DomainSeparatedHashInput v1
Truncation:          Forbidden
```

SHA-512/256 is the primary hash for consensus object identifiers,
domain-separated signing payload digests where a digest is required, state
commitments, block identifiers, transaction identifiers, receipt identifiers,
and protocol registries in the v0.1 profile.

Reserved profiles:

```text
0x0002 = hn-sha256-compat-v1      Proposed
0x0003 = hn-sha3-256-v1           Proposed
0x0004 = hn-shake256-v1           Proposed
0x0005 = hn-blake3-nonconsensus   Proposed
```

`Proposed` hash profiles are not valid for consensus commitments.

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

Initial domain tag binary representation:

```text
DomainTag
  u16 domain_tag_version = 1
  bounded UTF-8 string domain_name
```

Rules:

- `domain_name` must be ASCII lowercase.
- Allowed characters are `a-z`, `0-9`, and `.`.
- `domain_name` maximum length is 128 bytes.
- Domain tags are encoded with HNCS before hashing.
- Domain tag comparison is bytewise and case-sensitive.
- Domain names must not be normalized, case-folded, localized, or inferred from
  display text.

The complete v0.1 hash input is:

```text
DomainSeparatedHashInputV1
  u16 hash_profile_id
  DomainTag domain_tag
  bytes canonical_payload
```

`canonical_payload` is the HNCS encoding of the object being committed to. It
is length-delimited inside the hash input to avoid concatenation ambiguity.

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

The initial v0.1 digest length is 32 bytes for every active consensus hash
domain.

Initial domain digest lengths:

```text
address account          32 bytes
address contract         32 bytes
transaction id           32 bytes
transaction signing      32 bytes
receipt id               32 bytes
state leaf               32 bytes
state node               32 bytes
block header             32 bytes
block id                 32 bytes
p2p message id           32 bytes
algorithm registry       32 bytes
```

Address display length and checksum rules are not defined in this ADR. ADR-0003
may wrap or encode the 32-byte address digest, but it must not silently change
the underlying hash profile.

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

This section records evaluated alternatives and non-active reserved profiles.

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

## Accepted Initial Direction

HNChain adopts hash profile agility from genesis.

For the initial v0.1 profile:

- SHA-512/256 is the only active consensus hash algorithm.
- SHA-256 is reserved as a future compatibility profile, but inactive at
  genesis.
- SHA3-256 is reserved as a future conservative alternative, but inactive at
  genesis.
- SHAKE256 is reserved for future variable-output proof systems, but inactive at
  genesis.
- BLAKE3 is reserved for possible non-consensus content addressing first, and is
  not active for consensus.

### Rationale

SHA-512/256 provides a 256-bit digest using the SHA-512 family, is standardized
by NIST FIPS 180-4, and performs well on common 64-bit platforms. Selecting one
active consensus profile keeps the initial protocol surface smaller than
activating both SHA-2 and SHA-3 families.

SHA3-256 remains a strong conservative candidate, but activating it at genesis
alongside SHA-512/256 would increase implementation and test-vector surface
without a current protocol need.

BLAKE3 is not selected for consensus v0.1 because it is not a NIST FIPS
standard and should first be evaluated in non-consensus contexts.

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

Changing the algorithm, domain tag encoding, digest length, or input
construction of hash profile ID `0x0001` is a breaking protocol change.

## Related Specifications

- `docs/adr/ADR-0006-transaction-format.md`
- `docs/specs/core/hash-algorithms.md`
- `docs/specs/core/transaction-format.md`

## Deferred Decisions

- BLAKE3 role, if any, in non-consensus content addressing
- hash test vector file format
- approved cryptographic libraries
- future SHA3-256 activation rules
- future SHAKE256 proof-system profile

## References

- NIST FIPS 180-4: Secure Hash Standard
  https://csrc.nist.gov/pubs/fips/180-4/upd1/final
- NIST FIPS 202: SHA-3 Standard
  https://csrc.nist.gov/pubs/fips/202/final
- NIST SHA-3 update announcement
  https://www.nist.gov/news-events/news/2025/03/sha-3-nist-update-fips-202-and-revise-special-publication-800-185
- BLAKE3 specifications
  https://github.com/BLAKE3-team/BLAKE3-specs
