# ADR-0002: Cryptographic Identity

Status: Accepted

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model

Supersedes: None

## Context

HNChain requires a cryptographic identity model for accounts, validators,
network peers, governance participants, wallets, bridges, and smart contract
authorization.

Cryptographic identity must be specified before address format and canonical
serialization because key algorithms affect:

- public key length
- signature length
- signature verification rules
- key encoding
- algorithm identifiers
- key rotation semantics
- address derivation inputs
- long-term migration to new algorithms

HNChain must not assume that one signing algorithm will remain sufficient for
the lifetime of the network.

## Decision

HNChain cryptographic identity is algorithm-agile.

A cryptographic identity is represented by a versioned key descriptor, not by raw
public key bytes alone.

Conceptual structure:

```text
KeyDescriptor
  descriptor_version
  algorithm_id
  key_role
  public_key
  validity_rules
  metadata_commitment
```

Signatures are represented by a versioned signature envelope:

```text
SignatureEnvelope
  envelope_version
  algorithm_id
  key_reference
  signature
  verification_context
```

The address format is intentionally not defined in this ADR. Address derivation
is specified by ADR-0003.

The hash algorithm is intentionally not defined in this ADR. Hash functions and
domain separation are specified by ADR-0005.

The initial HNChain v0.1 cryptographic identity profile accepts Ed25519 as the
only active consensus signing algorithm.

Initial algorithm assignment:

```text
Algorithm ID:        0x0001
Algorithm Name:      Ed25519
Lifecycle:           Active
Public Key Length:   32 bytes
Signature Length:    64 bytes
Signature Mode:      PureEdDSA Ed25519
Specification:       RFC 8032
Active Roles:
  account_signing
  validator_consensus
  governance
```

The initial profile reserves but does not activate compatibility and
post-quantum algorithm identifiers:

```text
0x0002 = secp256k1_ecdsa_sha256        Proposed
0x0003 = ed448                         Proposed
0x0100 = ml_dsa_reserved               Proposed
0x0101 = slh_dsa_reserved              Proposed
```

`Proposed` algorithms are not valid for consensus signatures.

secp256k1 is not active at genesis. It may be activated later only through a
dedicated ADR or HNIP that specifies canonical public key encoding, canonical
signature encoding, low-S requirements, recovery identifier handling, role
limits, test vectors, and wallet compatibility rules.

Post-quantum signatures are not active at genesis. HNChain reserves algorithm
identifier space for them because key and signature envelopes must not assume
fixed Ed25519-sized payloads.

HNChain account identity supports multiple key descriptors structurally, but
the initial authorization profile requires each consensus authorization to
declare exactly one active signing key unless the owning object specification
defines a threshold or multisignature rule.

Threshold signatures, multisignature account policies, and key rotation
transactions are deferred to dedicated specifications. They must not be
implicitly defined by wallet software or SDK conventions.

## Normative Rules

### No Custom Cryptography By Default

HNChain must not create custom cryptographic algorithms for consensus use unless
the algorithm is accepted through a dedicated ADR, open external review, and a
long-term security analysis process.

The default policy is to use well-reviewed, openly specified cryptographic
standards and audited implementations.

Protocol novelty must come from system architecture, not unreviewed
cryptographic primitives.

### Algorithm Identifier

Every public key and signature must carry an explicit `algorithm_id`.

Nodes must not infer the algorithm from byte length, account type, address
prefix, RPC field shape, or wallet metadata.

### Key Roles

Key roles are explicit. Initial conceptual roles:

- `account_signing`
- `validator_consensus`
- `validator_network`
- `governance`
- `bridge_operator`
- `identity_recovery`

One physical key may be authorized for multiple roles only if protocol rules
explicitly allow it.

### Key Separation

HNChain should prefer separate keys for separate protocol roles.

Using the same key for account funds, validator consensus, validator network
identity, bridge operation, and governance is discouraged because compromise of
one operational surface compromises unrelated authority.

### Signature Context

Every signature must bind to a protocol-defined verification context.

The verification context must include at minimum:

- protocol name
- network identifier
- chain identifier
- object type
- object version
- signature purpose

Exact binary encoding is defined by ADR-0004.

### No Implicit Signing Payloads

Wallets, validators, RPC nodes, and bridges must not sign display strings,
JSON, or implementation-specific byte layouts for consensus authorization.

Consensus signatures are valid only over canonical signing payloads.

### Canonical Verification

Each signature algorithm must define:

- public key encoding
- signature encoding
- valid signature length
- canonicality rules
- rejection rules
- batch verification rules, if supported
- malleability restrictions
- test vectors

### Algorithm Lifecycle

Each algorithm has a lifecycle state:

```text
Proposed -> Active -> Deprecated -> Disabled
```

Lifecycle meanings:

- `Proposed`: specified but not valid for consensus use.
- `Active`: valid for configured roles.
- `Deprecated`: existing keys remain valid according to migration rules, but new
  keys should not be created for the role.
- `Disabled`: signatures are rejected except where historical verification rules
  explicitly require them.

Disabling an algorithm is a major security-sensitive protocol change and
requires governance activation rules.

### Crypto API Boundary

Core protocol logic depends on a Crypto API, not direct algorithm calls.

Conceptual boundary:

```text
Core
  -> Crypto API
  -> Algorithm Implementations
```

The Crypto API must expose explicit verification, hashing, key parsing, registry
lookup, and lifecycle checks.

The Crypto API must not hide consensus-critical behavior. Algorithm identifiers,
canonical encodings, rejection rules, and test vectors remain protocol-defined.

### Wallet Key Management Boundary

Seed phrases, hardware wallet UX, encrypted key storage, and recovery workflows
belong to wallet security specifications.

They must not silently define consensus behavior.

## Initial Algorithm Candidates

This section records evaluated alternatives and non-active reserved suites.

Candidate suites:

### Ed25519

Advantages:

- Small public keys and signatures.
- Fast verification in mature implementations.
- Deterministic signature scheme.
- Good fit for account and validator signatures.
- Specified by RFC 8032.

Disadvantages:

- Not post-quantum secure.
- Ecosystem interoperability with EVM-style wallets is weaker than secp256k1.
- Requires careful implementation to avoid accepting non-canonical encodings.

Recommended role if selected:

- primary `account_signing`
- primary `validator_consensus`

### Ed448

Advantages:

- Higher classical security margin than Ed25519.
- Specified by RFC 8032.

Disadvantages:

- Larger keys and signatures.
- Slower operations.
- Less common wallet and infrastructure support.

Recommended role if selected:

- high-assurance governance or long-lived protocol authority keys

### secp256k1

Advantages:

- Strong blockchain ecosystem support.
- Compatibility with Bitcoin and many EVM-oriented signing stacks.
- Mature implementations and hardware wallet support.
- Specified in SEC 2.

Disadvantages:

- ECDSA requires strict nonce handling; broken nonce generation leaks private
  keys.
- Signature malleability must be explicitly prohibited.
- Account and validator use may inherit compatibility complexity from existing
  ecosystems.
- Not post-quantum secure.

Recommended role if selected:

- compatibility account keys
- bridge-related keys

### ML-DSA

Advantages:

- NIST-standardized post-quantum digital signature family.
- Suitable candidate for long-term post-quantum migration planning.

Disadvantages:

- Larger keys and signatures than current elliptic-curve schemes.
- More expensive bandwidth and storage footprint.
- Ecosystem maturity in wallets and blockchain infrastructure is still evolving.

Recommended role if selected:

- future post-quantum or hybrid account and governance keys

### SLH-DSA

Advantages:

- NIST-standardized stateless hash-based signature family.
- Conservative backup class if lattice assumptions are weakened.

Disadvantages:

- Large signatures.
- Slower signing and verification compared with common elliptic-curve schemes.
- Not appropriate as a default high-throughput transaction signature without
  further performance analysis.

Recommended role if selected:

- fallback or high-assurance long-term authority keys

## Accepted Initial Direction

HNChain adopts algorithm agility from genesis.

For the initial v0.1 profile:

- Ed25519 is the primary and only active consensus signing suite for
  `account_signing`, `validator_consensus`, and `governance`.
- secp256k1 is reserved for future compatibility and bridge use, but inactive
  at genesis.
- Ed448 is reserved for future high-assurance roles, but inactive at genesis.
- ML-DSA and SLH-DSA identifier ranges are reserved for future post-quantum or
  hybrid profiles, but inactive at genesis.
- No post-quantum suite is active for high-throughput transaction signing until
  performance, implementation maturity, wallet support, and storage impact are
  measured.

This decision minimizes the active consensus surface while preserving a
versioned path for future algorithm migration.

### Rationale

Ed25519 is selected because it has compact 32-byte public keys, compact 64-byte
signatures, deterministic signing, mature implementations, and a clear public
specification.

secp256k1 is not activated initially because ECDSA canonicality, low-S
normalization, nonce failure risk, recovery identifier variants, and ecosystem
compatibility rules increase consensus complexity. These trade-offs may be
worth accepting later for bridge or wallet interoperability, but they should
not be included in the minimal genesis profile.

Post-quantum algorithms are not activated initially because key and signature
sizes, bandwidth costs, implementation maturity, and wallet support need
separate analysis. The protocol remains prepared for them through explicit
algorithm identifiers and variable-length envelopes.

## Security Considerations

Key compromise:

- Risk: a single key controls unrelated authorities.
- Mitigation: role-separated keys and account-level permission rules.

Algorithm downgrade:

- Risk: an attacker causes a node or wallet to verify under a weaker algorithm.
- Mitigation: explicit `algorithm_id`, role policy, and lifecycle checks.

Signature replay:

- Risk: a valid signature is reused across chains, networks, object types, or
  protocol versions.
- Mitigation: mandatory verification context.

Signature malleability:

- Risk: different signatures verify for the same intent.
- Mitigation: per-algorithm canonicality rules and rejection of non-canonical
  signatures.

Quantum migration:

- Risk: classical signatures become vulnerable before migration is operational.
- Mitigation: algorithm agility, lifecycle states, and future hybrid-key support.

Library risk:

- Risk: cryptographic implementation bugs cause fund loss or consensus splits.
- Mitigation: use audited, stable libraries; require test vectors; avoid custom
  cryptographic primitives.

Wallet recovery ambiguity:

- Risk: wallets derive different keys from the same recovery material.
- Mitigation: wallet specifications must define derivation standards, network
  separation, and test vectors.

## Compatibility

Adding a new algorithm can be backward-compatible only if:

- the algorithm receives a unique identifier
- all canonical encodings are specified
- role permissions are defined
- activation rules are defined
- unsupported nodes have deterministic rejection behavior

Changing verification behavior for an active algorithm is a major protocol
change.

Changing the accepted byte encoding, canonicality rules, or verification rules
for Ed25519 under algorithm ID `0x0001` is a breaking protocol change.

## Related Specifications

- `docs/adr/ADR-0003-address-format.md`
- `docs/specs/core/cryptographic-identity.md`
- `docs/specs/core/address-format.md`

## Deferred Decisions

- key rotation transaction semantics
- threshold and multisignature identity model
- hardware wallet compatibility requirements
- post-quantum migration profile
- cryptographic library selection
- seed phrase and wallet derivation standard
- hardware wallet signing payload requirements

## References

- RFC 8032: Edwards-Curve Digital Signature Algorithm
  https://datatracker.ietf.org/doc/html/rfc8032
- RFC 8410: Algorithm Identifiers for Ed25519, Ed448, X25519, and X448
  https://datatracker.ietf.org/doc/html/rfc8410
- SEC 2: Recommended Elliptic Curve Domain Parameters
  https://www.secg.org/sec2-v2.pdf
- NIST FIPS 204: Module-Lattice-Based Digital Signature Standard
  https://csrc.nist.gov/pubs/fips/204/final
- NIST Post-Quantum Cryptography project
  https://csrc.nist.gov/projects/post-quantum-cryptography
