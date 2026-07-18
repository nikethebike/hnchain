# HNChain Core RFC: Primitive Types

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0021-rust-workspace-policy.md`

## 1. Purpose

This RFC defines the initial primitive domain types for `hn-core`.

The goal is to prevent protocol-critical code from using ambiguous raw
integers, platform-dependent sizes, local time values, or free-form strings at
module boundaries.

## 2. Scope

This RFC defines:

- primitive type names
- semantic meaning
- valid ranges
- ordering rules
- canonical serialization expectations
- validation expectations
- implementation boundaries

This RFC does not define:

- account structures
- transaction structures
- block structures
- address format
- hash algorithm internals
- consensus algorithm details
- wall-clock synchronization rules

## 3. Design Principles

Core primitive types must be:

- deterministic
- compact
- explicit
- language-independent
- version-aware where required
- safe to use across module boundaries

Raw primitive aliases must not leak into public protocol-facing APIs when a
domain-specific type exists.

## 4. Primitive Type Set

The initial `hn-core` primitive set is:

```text
ProtocolVersion
ChainId
BlockHeight
Round
Epoch
UnixTimeMillis
ByteLength
AccountNonce
```

Additional primitive types require a new RFC or an amendment to this RFC.

## 5. Type Definitions

### 5.1 ProtocolVersion

Purpose:

- identifies the active protocol rule set
- supports compatibility checks
- appears in protocol objects where versioned interpretation is required

Domain:

```text
major: uint16
minor: uint16
patch: uint16
```

Rules:

- `major` changes identify incompatible protocol changes
- `minor` changes identify backward-compatible protocol extensions
- `patch` changes identify compatible fixes
- all fields are unsigned
- comparison is lexicographic by `major`, `minor`, then `patch`

Serialization:

```text
uint16 major
uint16 minor
uint16 patch
```

All integers are encoded according to HNCS canonical integer rules.

### 5.2 ChainId

Purpose:

- identifies a specific HNChain network
- prevents replay between networks
- appears in signed domain separation contexts

Domain:

```text
length: 1..32 bytes
value: ASCII lowercase letters, digits, hyphen
```

Rules:

- value must not be empty
- value must not exceed 32 bytes
- value must start with a lowercase ASCII letter
- value must end with a lowercase ASCII letter or digit
- consecutive hyphens are invalid
- uppercase and Unicode characters are invalid

Examples:

```text
hn-mainnet
hn-testnet-1
hn-devnet-local
```

Serialization:

```text
byte_length: uint8
bytes: byte[byte_length]
```

### 5.3 BlockHeight

Purpose:

- identifies the position of a block in the canonical chain
- supports storage indexing, consensus validation, and synchronization

Domain:

```text
uint64
```

Rules:

- genesis block height is `0`
- block height increases by exactly `1` for direct parent-child relationships
- maximum value is `2^64 - 1`
- overflow is invalid

Serialization:

```text
uint64
```

### 5.4 Round

Purpose:

- identifies a consensus attempt within a height or view
- supports leader election and vote validation

Domain:

```text
uint64
```

Rules:

- first round is `0`
- round numbers are scoped to a consensus height unless another consensus RFC
  specifies a different scope
- overflow is invalid

Serialization:

```text
uint64
```

### 5.5 Epoch

Purpose:

- identifies validator set and protocol scheduling periods
- supports staking, validator rotation, checkpoints, and governance activation

Domain:

```text
uint64
```

Rules:

- genesis epoch is `0`
- epoch transition rules are defined by consensus and validator-set RFCs
- epoch arithmetic must be checked for overflow

Serialization:

```text
uint64
```

### 5.6 UnixTimeMillis

Purpose:

- represents protocol-facing timestamps where required
- avoids platform-specific time representations

Domain:

```text
uint64 milliseconds since 1970-01-01T00:00:00Z
```

Rules:

- value is UTC-based Unix time in milliseconds
- leap seconds are not represented separately
- local timezone is never part of the value
- system wall-clock time must not be read by deterministic state transition code
- consensus rules must define when external time may be sampled

Serialization:

```text
uint64
```

### 5.7 ByteLength

Purpose:

- represents bounded byte lengths in protocol and storage validation
- prevents platform-dependent `usize` from crossing protocol boundaries

Domain:

```text
uint32
```

Rules:

- used for protocol-visible byte lengths unless a narrower field is specified
- conversion from host memory sizes must be checked
- overflow is invalid
- module-specific maximums may be lower than `2^32 - 1`

Serialization:

```text
uint32
```

### 5.8 AccountNonce

Purpose:

- orders account-originated transactions
- prevents replay within account transaction domains

Domain:

```text
uint64
```

Rules:

- initial account nonce is `0`
- nonce increments are defined by transaction and account-state specifications
- skipped nonce handling is defined by mempool and transaction validation RFCs
- overflow is invalid

Serialization:

```text
uint64
```

## 6. Integer Encoding

All integer serialization in this RFC delegates to HNCS canonical integer rules.

Until HNCS is accepted, implementations must not treat Rust memory layout,
endianness of the host CPU, serde defaults, or debug formatting as protocol
serialization.

## 7. Ordering

Ordering rules:

- numeric primitives use numeric ordering
- `ProtocolVersion` uses semantic lexicographic ordering
- `ChainId` uses bytewise ordering only for deterministic maps and tests

Ordering must not depend on locale, Unicode normalization, or platform string
collation.

## 8. Validation

Each primitive must be constructed through validation-aware APIs.

Invalid values must return typed validation errors.

Rules:

- invalid external input must not panic
- narrowing conversions must be checked
- arithmetic must be checked where overflow is possible
- parsing must reject trailing bytes unless the caller explicitly accepts a
  framed format
- display formatting must not be used for canonical serialization

## 9. Rust Implementation Guidance

The Rust reference implementation should model these primitives as newtypes.

Conceptual example:

```text
BlockHeight(uint64)
Round(uint64)
Epoch(uint64)
AccountNonce(uint64)
ByteLength(uint32)
ChainId(validated bytes/string)
ProtocolVersion { major, minor, patch }
UnixTimeMillis(uint64)
```

Public constructors should validate invariants.

Raw inner values may be exposed through explicit accessors, but implicit
cross-domain conversions should be avoided.

## 10. Prohibited Behavior

The following are prohibited at protocol-facing boundaries:

- platform-dependent integer sizes such as `usize` or `isize`
- implicit integer narrowing
- local timezone timestamps
- floating-point timestamps
- locale-dependent string comparison
- deriving protocol serialization from memory layout
- accepting unvalidated `String` values as `ChainId`
- using wall-clock time inside deterministic state transitions

## 11. Compatibility

These primitive definitions are intended to remain backward compatible.

Changing the binary representation of any primitive is a protocol-breaking
change unless introduced through an explicitly versioned format.

Adding a new primitive type is backward-compatible only if it does not change
existing object formats or validation semantics.

## 12. Security Considerations

Replay risk:

- Risk: missing or ambiguous `ChainId` values allow replay across networks.
- Mitigation: validated chain identifiers and signed domain separation.

Overflow risk:

- Risk: unchecked arithmetic changes validation results across implementations.
- Mitigation: checked arithmetic and explicit overflow errors.

Serialization ambiguity:

- Risk: different implementations encode identical values differently.
- Mitigation: HNCS-only canonical encoding and conformance test vectors.

Time nondeterminism:

- Risk: local clocks influence deterministic state transitions.
- Mitigation: time sampling is restricted to consensus-defined boundaries.

String ambiguity:

- Risk: Unicode, locale, or normalization differences split consensus behavior.
- Mitigation: restricted ASCII `ChainId` grammar.

## 13. Required Test Vectors

The implementation phase must add test vectors for:

- minimum and maximum numeric values
- overflow rejection
- `ProtocolVersion` ordering
- valid and invalid `ChainId` values
- canonical byte encodings
- rejected trailing bytes
- checked conversions from host memory sizes

Test vectors must be language-independent and stored outside Rust-only unit
tests when they define protocol behavior.

## 14. Open Decisions

- exact HNCS integer byte order before ADR-0004 is accepted
- whether `ProtocolVersion` should include pre-release metadata outside
  consensus objects
- exact error taxonomy for primitive validation
- exact location and format for conformance test vectors
- whether `ChainId` should reserve well-known values before mainnet

## 15. Related Documents

- `docs/specs/core/canonical-serialization.md`
- `docs/specs/core/hash-algorithms.md`
- `docs/specs/core/account-state.md`
- `docs/adr/ADR-0021-rust-workspace-policy.md`
