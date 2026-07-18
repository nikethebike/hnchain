# ADR-0003: Address Format

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity

Supersedes: None

## Context

HNChain requires addresses for accounts, validators, smart contracts, protocol
modules, bridge objects, identity records, and future namespaces.

Address format depends on cryptographic identity because key algorithms affect
public key encoding, key descriptors, key rotation, signature suites, and
possible post-quantum migration paths.

The address format must support:

- algorithm agility
- network and chain separation
- account type separation
- contract and protocol module addresses
- human-readable encoding with error detection
- canonical binary representation for consensus
- future migration without reinterpreting existing addresses

## Decision

HNChain addresses are versioned protocol identifiers.

Consensus uses a canonical binary `AddressPayload`. Human-readable address
strings are an external representation of that payload.

Conceptual binary structure:

```text
AddressPayload
  address_version
  address_namespace
  network_id
  derivation_scheme
  address_body
  checksum_profile
```

Conceptual text structure:

```text
hrp + separator + encoded(AddressPayload)
```

The exact canonical binary encoding is defined by ADR-0004.

The exact hash function used by derivation schemes is defined by ADR-0005.

## Normative Rules

### Binary Payload Is Normative

The canonical binary `AddressPayload` is the consensus object.

Textual address strings are not used for state hashing, signature payloads,
state tree keys, or consensus equality.

### Versioned Address Format

Every address includes `address_version`.

Nodes must not infer address version from string length, prefix length, payload
length, checksum behavior, or account type.

### Namespace Separation

Every address includes `address_namespace`.

Initial conceptual namespaces:

- `account`
- `contract`
- `validator`
- `protocol`
- `bridge`
- `identity`

Namespaces prevent accidental reuse of the same bytes for different protocol
domains.

### Network Separation

Every address binds to a `network_id`.

An address valid on one HNChain network must not be silently valid on another
network unless cross-network behavior is explicitly specified.

Human-readable prefixes are useful, but they are not sufficient network
separation for consensus. The network identifier must be inside the canonical
payload.

### Derivation Scheme Separation

Every address includes `derivation_scheme`.

Nodes must not infer derivation scheme from `algorithm_id` alone. Multiple
address derivation schemes may exist for the same key algorithm.

### Fixed Consensus Equality

Two addresses are equal only if their canonical binary payloads are byte-for-byte
equal after canonical decoding.

Case folding, Unicode normalization, whitespace trimming, or display formatting
must not affect consensus equality.

### No Raw Public Keys As Addresses

Raw public keys are not addresses.

An account may rotate or add keys without changing its address if account rules
support stable identity. Conversely, a new address may be derived from a key
descriptor if the chosen derivation scheme defines that behavior.

### Human-Readable Encoding

HNChain should use a Bech32m-style human-readable encoding for user-facing
addresses unless rejected by later implementation analysis.

Reasons:

- lower risk of ambiguous characters than mixed-case encodings
- built-in checksum suitable for copy and paste workflows
- human-readable prefix support
- existing implementation experience in blockchain systems

The checksum protects address entry and transport. It is not a cryptographic
integrity mechanism and must not replace canonical hashing or signature
verification.

## Initial Address Namespaces

### Account Address

Used for user accounts and account-controlled state.

Initial derivation direction:

```text
account_address = ADDRESS_DERIVE(
  network_id,
  namespace = account,
  derivation_scheme,
  key_descriptor_or_identity_commitment
)
```

Exact derivation waits for ADR-0004 and ADR-0005.

### Contract Address

Used for smart contract instances.

Contract address derivation must bind to:

- creator or deployer account
- deployment nonce or unique deployment input
- code commitment
- network identifier
- contract namespace

### Validator Address

Used for validator identity inside consensus and staking.

Validator address must not be treated as equivalent to account address unless a
specific binding is present in validator state.

### Protocol Address

Used for protocol-owned modules such as treasury, governance system contracts,
staking, slashing, bridge registries, and future native modules.

Protocol addresses must be reserved by genesis or governance-controlled
activation rules.

### Bridge Address

Used for bridge-related accounting and external chain commitments.

Bridge addresses must bind to explicit bridge namespace and chain identifiers to
avoid cross-chain replay and asset confusion.

### Identity Address

Used for HN Identity records if identity becomes a distinct protocol namespace.

Identity address semantics must not be overloaded onto account addresses without
an explicit binding model.

## Recommended Initial Profile

This ADR proposes, but does not yet accept, the following initial profile:

- canonical binary payload for consensus
- Bech32m-style text encoding for wallets, CLI, RPC, and explorer
- lowercase HRP
- separate HRPs for mainnet, testnet, and local development networks
- network identifier inside the binary payload
- 32-byte address body for initial classical account and contract addresses
- variable-length payload support for future address versions

The 32-byte address body is a profile choice, not a universal invariant.
Post-quantum, identity-commitment, or special protocol addresses may require
different body lengths in later versions.

## Rejected Options

### Address Equals HASH(PublicKey)

Advantages:

- simple
- compact
- common in blockchain systems

Disadvantages:

- couples address identity to one key representation
- complicates key rotation
- hides algorithm choice
- makes post-quantum migration harder
- does not naturally support contract, protocol, bridge, and identity namespaces

Rejected as the general HNChain address model.

It may still appear as one derivation scheme inside the versioned address
framework.

### Raw Public Key Address

Advantages:

- avoids hash collision discussion for key-derived addresses
- direct verification of address-to-key relation

Disadvantages:

- large addresses
- exposes key material before first use
- poor fit for post-quantum public key sizes
- weak UX
- difficult namespace separation

Rejected.

### Hex String Address

Advantages:

- simple tooling
- familiar to EVM users

Disadvantages:

- weak error detection unless an additional checksum scheme is added
- less ergonomic for manual transfer
- case-checksum schemes introduce case-handling pitfalls

Rejected for the default user-facing format.

## Security Considerations

Cross-network replay:

- Risk: an address from one network is accepted on another network.
- Mitigation: include `network_id` in canonical payload and signing context.

Namespace confusion:

- Risk: account, contract, validator, protocol, bridge, or identity addresses are
  interpreted interchangeably.
- Mitigation: mandatory `address_namespace`.

Algorithm migration failure:

- Risk: addresses cannot survive key algorithm migration.
- Mitigation: avoid raw-key addresses and require `derivation_scheme`.

Checksum misuse:

- Risk: UI checksum is treated as cryptographic integrity.
- Mitigation: checksum is only an input error detector; consensus uses canonical
  bytes, hashes, and signatures.

Unicode and display attacks:

- Risk: visually similar characters or normalization change displayed address
  meaning.
- Mitigation: restrict text encoding alphabet and reject non-canonical display
  forms.

Address truncation:

- Risk: UI displays shortened addresses and users approve the wrong target.
- Mitigation: wallets and explorers must use documented display rules and show
  enough information for high-value operations.

## Compatibility

Adding a new address namespace or derivation scheme can be backward-compatible
only if:

- it has a unique identifier
- canonical encoding is specified
- unsupported nodes have deterministic rejection behavior
- state transition rules for the namespace are defined

Changing equality, decoding, checksum validation, or derivation semantics for an
existing address version is a major protocol change.

## Related Specifications

- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/specs/core/address-format.md`
- `docs/specs/core/canonical-serialization.md`

## Open Decisions

- final mainnet, testnet, and devnet human-readable prefixes
- binary size and type of `network_id`
- initial account address body length
- address body derivation function
- whether addresses bind directly to key descriptors or identity commitments
- contract address derivation inputs
- protocol namespace reservation process
- bridge chain identifier format
- display and truncation requirements for wallets and explorers

## References

- BIP 173: Base32 address format for native v0-16 witness outputs
  https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki
- BIP 350: Bech32m format
  https://github.com/bitcoin/bips/blob/master/bip-0350.mediawiki
- CAIP-2: Blockchain ID Specification
  https://standards.chainagnostic.org/CAIPs/caip-2
