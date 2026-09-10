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
- `docs/adr/ADR-0005-hash-algorithms.md`

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
```

Field meanings:

- `address_version`: version of the address payload format.
- `address_namespace`: protocol namespace of the address. `uint8`, closed
  registry for `address_version = 1` (§4).
- `network_id`: canonical network identifier. `uint16` (§5).
- `derivation_scheme`: identifier for address derivation rules.
- `address_body`: derived identifier bytes. 32 bytes for every namespace
  under `address_version = 1` (ADR-0003, "Address Body Length").

`AddressPayload` deliberately does not carry `chain_id` or
`checksum_profile` (ADR-0003, Decision). `chain_id` (which HNChain chain
lineage) is a different field from `network_id` (which environment) owned
elsewhere (ADR-0008); addresses are chain-lineage-agnostic. A checksum
protects text-encoding entry and transport, not consensus identity, so it
lives entirely in the text representation (§7), not in this payload — if it
were a payload field, two payloads naming the same object but checksummed
differently would count as different addresses under Address Equality (§8).

The canonical field encoding is defined by the serialization specification.

## 4. Address Namespaces

`address_namespace` is `uint8`. The registry for `address_version = 1` is
closed (ADR-0003, "Namespace Separation"):

```text
0x00  reserved, invalid for committed addresses
0x01  account
0x02  contract
0x03  validator
0x04  protocol
0x05  bridge
0x06  identity
```

Namespace rules:

- Namespaces are consensus-relevant.
- Unknown namespaces are rejected unless activated by protocol rules.
- An address from one namespace must not be accepted where another namespace is
  required.
- Adding a namespace requires a new `address_version`, not an addition to
  the `address_version = 1` registry.

The `protocol` namespace's `address_version = 1` genesis reservation list
(treasury, governance, staking, slashing, bridge registry) is fixed at
genesis and enumerated in the genesis specification, not here. Whether more
protocol modules can be reserved after genesis without a hard fork depends
on a governance process that does not exist yet (ADR-0003, Deferred
Decisions).

## 5. Network Identifier

`network_id` prevents accidental cross-network address reuse.

`network_id` is `uint16` (ADR-0003, "Network Separation"), split into a
registered range and a self-assigned devnet range:

```text
0x0000           reserved, invalid
0x0001           mainnet
0x0002           testnet
0x0003..0x7FFF   reserved for future centrally registered networks
0x8000..0xFFFF   devnet range: self-assigned per devnet instance, not
                 centrally registered
```

`uint16`, not `uint8` like `address_namespace`: devnets are expected to run
many concurrent disposable instances, and a single shared devnet value
would leave two independently spun-up devnets indistinguishable at the
address level.

Network identifier rules:

- `network_id` is part of the canonical address payload.
- `network_id` is part of signature verification context.
- Wallets and RPC clients must reject mismatched network identifiers.
- The human-readable prefix duplicates network information for display, but
  is not consensus-authoritative and does not replace `network_id`; wallets,
  CLI tools, and explorers must verify the decoded `network_id` matches the
  network the presented HRP claims (§7) and reject or warn on mismatch.

## 6. Derivation Scheme

`derivation_scheme` defines how `address_body` is produced.

Candidate derivation inputs:

- key descriptor commitment
- cryptographic identity commitment
- creator address
- deployment nonce
- code commitment
- protocol namespace identifier
- external chain identifier and external account reference (bridge
  namespace only — see below)

For the `bridge` namespace, `address_body` is a hash commitment over
`(external_chain_id, external_account_reference)` under
`HASH_PROFILE_0x0001`, not those raw external bytes embedded directly; this
keeps the bridge body the same 32-byte length as every other namespace
(ADR-0003, "Bridge Address"). The external chain identifier's own format is
still open (§11).

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
hrp + separator + encoded_payload + checksum
```

The HRP encodes `network_id` only — never `address_namespace` or any other
payload field (ADR-0003, "HRP scope"). Wallets and explorers read
`address_namespace` from the decoded payload, not from the prefix, so it
needs no HRP of its own. There is one HRP for mainnet, one for testnet, and
one shared HRP for the whole self-assigned devnet `network_id` range — not
one HRP per devnet instance (ADR-0003, "Recommended Initial Profile" and
"HRP-network_id consistency"); checking a decoded `network_id` against its
HRP is exact-value matching for mainnet/testnet and range membership for
devnet.

The checksum is part of this text structure, not of `AddressPayload` (§3):
it protects entry and transport, and cannot affect consensus equality (§8)
by construction.

Rules:

- text encoding must have error detection
- text encoding must reject mixed-case ambiguity
- text encoding must reject non-canonical forms
- decoding must produce exactly one `AddressPayload`
- consensus must not hash or compare text addresses
- the decoded `network_id` must be checked against the presented HRP before
  display or acceptance; a mismatch must be rejected or prominently warned
  on, not silently accepted (ADR-0003, "HRP spoofing")

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
- Wallets, CLI tools, and explorers must verify decoded `network_id` against
  the presented HRP and reject or warn on mismatch ("HRP spoofing").

## 11. Open Architecture Decisions

Resolved by ADR-0003 and removed from this list: final network identifier
format (`uint16`, §5), final namespace numeric identifiers (`uint8`, §4),
final address body length (32 bytes uniform, §3). Protocol address
reservation rules are resolved for the `address_version = 1` genesis list
(§4) but the post-genesis extension process remains open, deferred pending
a governance ADR that does not exist yet (ADR-0003, Deferred Decisions).

Still open:

- final HRPs (the literal prefix strings)
- final derivation scheme identifiers
- contract address derivation rules
- bridge address external chain identifier format (how `external_chain_id`
  itself is encoded before being hashed into `address_body`; the hashing
  itself is resolved, §6)
- text address codec test vectors
