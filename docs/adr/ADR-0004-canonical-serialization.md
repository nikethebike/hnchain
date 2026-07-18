# ADR-0004: Canonical Serialization

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0003: Address Format

Supersedes: None

## Context

HNChain requires a canonical binary serialization format for consensus objects,
signing payloads, state roots, transaction identifiers, block identifiers,
storage records, proofs, and network messages.

Canonical serialization must be specified before hash algorithms, transaction
format, state tree, and block format because those components commit to
serialized bytes.

The format must be deterministic across languages, operating systems, CPU
architectures, compiler versions, and independent implementations.

## Decision

HNChain defines its own canonical binary serialization profile for consensus
data.

This profile is called HNCS:

```text
HNCS = HNChain Canonical Serialization
```

HNCS is a strict, schema-driven, length-delimited binary format. It is not a
general-purpose object serialization system.

Consensus objects must define their HNCS schema before implementation.

JSON, gRPC, REST, CLI display formats, wallet display formats, database layouts,
and in-memory structs are not canonical consensus representations.

## Rationale

HNChain needs a serialization format with exactly one valid byte representation
for each consensus value.

Existing formats such as Protocol Buffers, CBOR, RLP, SCALE, and SSZ provide
useful design references, but adopting any of them as-is creates trade-offs:

- Protocol Buffers deterministic serialization is not a cross-language canonical
  consensus commitment by default.
- CBOR supports deterministic profiles, but its generic data model is broader
  than HNChain consensus requires.
- RLP is compact and proven in Ethereum execution, but it lacks typed schema
  structure and leaves atomic type semantics to higher-level protocols.
- SSZ is attractive for merkleization, but its design is closely tied to
  Ethereum consensus data structures and would still require an HNChain profile.

HNCS should borrow conservative ideas from existing systems but remain a small
protocol-specific format with explicit schemas and test vectors.

## Normative Rules

### One Value, One Encoding

Every consensus value has exactly one valid HNCS encoding.

Decoders must reject non-canonical encodings.

### Schema Required

HNCS encoding is valid only relative to a declared schema.

Self-describing field names, runtime reflection, unbounded maps, and implicit
type inference are not part of consensus encoding.

### Versioned Objects

Every top-level protocol object includes an explicit object version.

Version interpretation is specified by the owning protocol document.

### Field Order

Struct fields are encoded in schema order.

Field order must not depend on source language declaration order unless that
order is explicitly generated from the protocol schema.

### Integer Encoding

Consensus integers use fixed-width unsigned or signed integer types selected by
schema.

Integer byte order is little-endian unless a future accepted serialization
profile changes this before implementation.

Variable-length integers are rejected for consensus core objects in the initial
profile because they add extra canonicality and overflow surface.

### Floating Point

Floating point values are forbidden in consensus state, consensus transaction
payloads, consensus signatures, state roots, block headers, and VM deterministic
execution.

### Boolean Encoding

Boolean values are encoded as one byte:

```text
false = 0x00
true  = 0x01
```

Any other byte value is invalid.

### Byte Sequences

Variable-length byte sequences are encoded as:

```text
u32_length || bytes
```

The maximum allowed length is defined by the schema.

Length fields must use canonical fixed-width encoding and must match the exact
payload length.

### Strings

Consensus strings are discouraged.

When unavoidable, strings must be UTF-8, length-delimited, normalized by
protocol rules if normalization is allowed, and bounded by schema.

Consensus equality must never depend on locale, case folding, or display
normalization.

### Lists

Lists are encoded as:

```text
u32_count || element_0 || element_1 || ... || element_n
```

The schema defines maximum element count.

List order is consensus-relevant.

### Sets

Sets are encoded as sorted lists.

Sorting is bytewise ascending order of each element's canonical HNCS encoding.

Duplicate canonical encodings are invalid.

### Maps

Maps are encoded as sorted key-value entries.

Sorting is bytewise ascending order of each key's canonical HNCS encoding.

Duplicate canonical key encodings are invalid.

Unbounded maps are forbidden in consensus objects.

### Optional Fields

Optional fields must be explicitly represented by a presence byte:

```text
0x00 = absent
0x01 = present
```

If present, the value immediately follows the presence byte.

Any other presence byte is invalid.

Optional fields must have deterministic defaults defined by schema.

### Enums

Enums are encoded using fixed-width unsigned integer discriminants defined by
schema.

Unknown enum discriminants are invalid unless the schema explicitly defines
preservation behavior for non-consensus extension data.

### Extensions

Extension payloads must be length-delimited and versioned.

Unknown critical extensions are invalid.

Unknown non-critical extensions may be preserved only if the owning
specification defines preservation and hashing rules.

### Rejection On Trailing Bytes

Decoders must reject trailing bytes after a complete value.

### Resource Limits

Every variable-length field must have an explicit maximum size.

Decoders must enforce limits before allocation where possible.

## Initial Type Set

The initial HNCS profile supports:

- `bool`
- `u8`, `u16`, `u32`, `u64`, `u128`
- `i8`, `i16`, `i32`, `i64`, `i128`
- fixed byte arrays
- bounded byte sequences
- bounded UTF-8 strings
- structs
- enums
- optional values
- bounded lists
- bounded sets
- bounded maps

Not supported in consensus:

- floating point numbers
- NaN or infinity values
- unbounded collections
- object references
- pointers
- local timestamps
- host-dependent integer sizes
- implementation-defined structs

## Compatibility

Backward-compatible schema changes may:

- add optional fields with deterministic defaults if the owning object version
  permits it
- add non-critical extension records with defined preservation rules
- add new object versions with explicit upgrade behavior

Backward-incompatible changes include:

- changing field order
- changing integer width
- changing byte order
- changing default values
- changing maximum lengths in a way that changes accepted consensus data
- changing map or set ordering
- changing extension criticality behavior
- accepting previously invalid encodings

Backward-incompatible changes require a major protocol upgrade.

## Related Specifications

- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/specs/core/canonical-serialization.md`
- `docs/specs/core/hash-algorithms.md`

## Rejected Options

### JSON Canonicalization

Advantages:

- human-readable
- easy debugging
- broad tooling

Disadvantages:

- inefficient for consensus data
- high risk of Unicode, number, whitespace, and ordering pitfalls
- poor fit for binary keys, signatures, hashes, and proofs

Rejected for consensus serialization.

JSON may be used for RPC views only when it is explicitly documented as
non-consensus representation.

### Protocol Buffers As Consensus Encoding

Advantages:

- mature tooling
- strong schema ecosystem
- broad language support

Disadvantages:

- deterministic serialization is not sufficient as a canonical cross-language
  consensus commitment by default
- unknown-field behavior and schema evolution require additional restrictions
- field ordering and presence semantics need protocol-specific rules

Rejected as the direct consensus encoding.

Protocol Buffers may still be used for gRPC transport if messages are not used
as canonical consensus bytes.

### Generic Deterministic CBOR

Advantages:

- standardized binary data model
- deterministic encoding profile exists
- good general-purpose interoperability

Disadvantages:

- broader data model than needed for consensus
- requires an HNChain-specific profile to exclude ambiguous or unnecessary types
- map ordering and tag handling need strict constraints

Rejected as the direct consensus encoding for now.

CBOR remains a useful reference for deterministic encoding design.

### RLP

Advantages:

- compact
- proven in Ethereum execution clients
- simple byte and list model

Disadvantages:

- weak schema expressiveness
- atomic type semantics are external to the encoding
- less suitable for explicit versioned protocol objects without an additional
  schema layer

Rejected as the direct HNChain consensus encoding.

## Security Considerations

Consensus split:

- Risk: independent implementations encode or decode the same object
  differently.
- Mitigation: strict schemas, canonical encodings, rejection rules, and test
  vectors.

Malleable signing payload:

- Risk: multiple byte encodings authorize the same user intent.
- Mitigation: one value, one encoding and no signing of JSON/display strings.

Resource exhaustion:

- Risk: malicious payloads force large allocation or slow decoding.
- Mitigation: explicit maximum sizes and allocation-before-limit rejection.

Unknown field ambiguity:

- Risk: unsupported fields are preserved, dropped, or hashed differently.
- Mitigation: extension criticality flags and explicit preservation rules.

Integer ambiguity:

- Risk: overflow or width mismatch changes state transitions.
- Mitigation: fixed-width integers and overflow-safe arithmetic.

String ambiguity:

- Risk: Unicode normalization or locale behavior changes equality.
- Mitigation: avoid consensus strings and define strict UTF-8 handling when
  strings are unavoidable.

## Implementation Requirements

Before any consensus implementation is accepted:

- HNCS schemas must be documented for every consensus object.
- Golden test vectors must be created for valid encodings.
- Negative test vectors must be created for non-canonical encodings.
- Decoders must reject trailing bytes.
- Decoders must enforce length limits.
- Cross-language test vectors must be supported before a second implementation
  is considered compatible.

## Open Decisions

- final byte order before implementation
- schema definition language
- numeric identifiers for primitive and compound types
- exact extension record encoding
- maximum length defaults
- test vector file format
- code generation strategy for Rust or Go
- whether non-consensus network messages reuse HNCS or use transport-specific
  schemas

## References

- RFC 8949: Concise Binary Object Representation
  https://www.rfc-editor.org/info/rfc8949/
- Protocol Buffers Encoding
  https://protobuf.dev/programming-guides/encoding/
- Ethereum RLP
  https://ethereum.org/developers/docs/data-structures-and-encoding/rlp/
