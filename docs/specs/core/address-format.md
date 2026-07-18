# HNChain Core Specification: Address Format

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## 1. Scope

This document specifies the conceptual HNChain address model.

It defines address payload fields, namespaces, derivation boundaries,
human-readable representation, module boundaries, and security requirements.

This document does not define canonical binary encoding, hash algorithms, final
derivation functions, transaction fields, state tree keys, or RPC schemas.

This specification is constrained by:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0003-address-format.md`

## 2. Design Goals

- Keep consensus address identity independent from UI string formatting.
- Support multiple cryptographic algorithms and future derivation schemes.
- Prevent namespace and network confusion.
- Support account, contract, validator, protocol, bridge, and identity domains.
- Provide human-readable addresses with input error detection.
- Preserve future compatibility with post-quantum and multi-key identities.

## 3. Address Payload

Conceptual structure:

```text
AddressPayload
  address_version
  address_namespace
  network_id
  derivation_scheme
  address_body
  checksum_profile
```

Field meanings:

- `address_version`: version of the address payload format.
- `address_namespace`: protocol namespace of the address.
- `network_id`: canonical network identifier.
- `derivation_scheme`: identifier for address derivation rules.
- `address_body`: derived identifier bytes.
- `checksum_profile`: text encoding checksum profile for external display.

The canonical field encoding is defined by the serialization specification.

## 4. Address Namespaces

Initial namespaces:

```text
account
contract
validator
protocol
bridge
identity
```

Namespace rules:

- Namespaces are consensus-relevant.
- Unknown namespaces are rejected unless activated by protocol rules.
- An address from one namespace must not be accepted where another namespace is
  required.

## 5. Network Identifier

`network_id` prevents accidental cross-network address reuse.

Network identifier rules:

- `network_id` is part of the canonical address payload.
- `network_id` is part of signature verification context.
- Wallets and RPC clients must reject mismatched network identifiers.
- The human-readable prefix may duplicate network information, but does not
  replace `network_id`.

The exact network identifier format remains open.

## 6. Derivation Scheme

`derivation_scheme` defines how `address_body` is produced.

Candidate derivation inputs:

- key descriptor commitment
- cryptographic identity commitment
- creator address
- deployment nonce
- code commitment
- protocol namespace identifier
- bridge chain identifier

Rules:

- Derivation must be deterministic.
- Derivation must include domain separation.
- Derivation must not depend on display strings.
- Derivation must not infer algorithm from public key length.
- Derivation must reject non-canonical key descriptors.

## 7. Human-Readable Address

Human-readable address is an external representation of `AddressPayload`.

Recommended conceptual structure:

```text
hrp + separator + encoded_payload
```

Rules:

- text encoding must have error detection
- text encoding must reject mixed-case ambiguity
- text encoding must reject non-canonical forms
- decoding must produce exactly one `AddressPayload`
- consensus must not hash or compare text addresses

HNChain should use Bech32m-style encoding unless implementation analysis
rejects it.

## 8. Address Equality

Address equality is defined only by canonical binary payload equality.

```text
address_a == address_b
  iff canonical_bytes(address_a) == canonical_bytes(address_b)
```

String formatting, letter case, whitespace, Unicode normalization, or display
truncation must not affect equality.

## 9. Module Boundaries

```text
Wallet / CLI / Explorer
      |
      v
Text Address Codec
      |
      v
AddressPayload
      |
      v
Validation Layer -----> Cryptographic Identity
      |
      v
State Transition Engine
```

Boundary rules:

- Wallets and CLI encode and decode text addresses, but do not define consensus
  address semantics.
- RPC may expose both canonical structured fields and text address fields.
- State transition logic uses canonical address payloads.
- Storage may index by canonical address bytes, but does not define address
  derivation.
- Cryptography defines key descriptors; address format defines identifiers
  derived from or bound to those descriptors.

## 10. Security Requirements

- Unknown address versions are rejected unless specified by protocol upgrade
  rules.
- Unknown namespaces are rejected unless specified by protocol upgrade rules.
- Text decoding must reject invalid checksums.
- Text decoding must reject non-canonical casing.
- Address derivation must use canonical input bytes.
- Address derivation must include domain separation.
- Wallet display must prevent silent network or namespace confusion.

## 11. Open Architecture Decisions

- final HRPs
- final network identifier format
- final namespace numeric identifiers
- final derivation scheme identifiers
- final address body length for account addresses
- contract address derivation rules
- protocol address reservation rules
- bridge address chain identifier format
- text address codec test vectors
