# HNChain Core Specification: Canonical Serialization

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## 1. Scope

This document specifies the conceptual HNChain Canonical Serialization profile,
abbreviated as HNCS.

HNCS defines canonical binary representation for consensus objects.

This document does not define hash algorithms, transaction schema, state tree
layout, block schema, RPC JSON format, gRPC transport encoding, or database
physical layout.

This specification is constrained by:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0004-canonical-serialization.md`

## 2. Design Goals

- One valid binary encoding per consensus value.
- Language-independent encoding and decoding.
- Strict rejection of non-canonical data.
- Bounded allocation and decoding work.
- Schema-driven evolution.
- Compatibility with cryptographic signing, hashing, storage records, and state
  proofs.

## 3. HNCS Data Model

HNCS supports a small typed data model:

```text
Primitive
  bool
  fixed-width integers
  fixed byte arrays
  bounded byte sequences
  bounded UTF-8 strings

Compound
  struct
  enum
  optional
  bounded list
  bounded set
  bounded map
```

HNCS does not support consensus floating point values.

HNCS does not define accounts, addresses, transactions, blocks, consensus
messages, validators, economics, RPC, P2P behavior, database schemas, or
application data models.

## 4. Encoding Rules

### 4.1 Boolean

```text
false = 0x00
true  = 0x01
```

Other values are invalid.

### 4.2 Fixed-Width Integers

Supported integer types:

```text
u8, u16, u32, u64, u128
i8, i16, i32, i64, i128
```

Initial profile byte order:

```text
little-endian
```

Host-size integers such as C `int`, Rust `usize`, or Go `int` are forbidden in
consensus schemas.

### 4.3 Fixed Byte Arrays

Fixed byte arrays are encoded as raw bytes with no length prefix.

The length is defined by schema.

### 4.4 Bounded Byte Sequences

Variable-length bytes are encoded as:

```text
u32_length || bytes
```

The schema defines a maximum length.

`u32_length` must equal the exact number of bytes that follow.

### 4.5 Bounded UTF-8 Strings

Strings are encoded as:

```text
u32_length || utf8_bytes
```

Rules:

- bytes must be valid UTF-8
- maximum length is defined by schema
- no locale-sensitive comparison is allowed
- Unicode normalization is not performed by the initial HNCS profile
- schemas that require normalized text must define that requirement before HNCS
  encoding

Consensus schemas should avoid strings where byte identifiers are sufficient.

### 4.6 Structs

Struct fields are encoded in schema order.

```text
field_0 || field_1 || ... || field_n
```

No field tags are encoded in the initial profile.

Schema versioning defines how fields are added or removed.

### 4.7 Enums

Enums are encoded as a fixed-width unsigned discriminant followed by variant
payload if the selected variant has one.

The discriminant width is defined by schema.

Unknown discriminants are invalid unless an extension specification defines
preservation behavior.

### 4.8 Optional Values

Optional values are encoded as:

```text
presence || value_if_present
```

Presence values:

```text
0x00 = absent
0x01 = present
```

Any other presence byte is invalid.

If the presence byte is `0x00`, no value bytes may follow for that optional
value.

If the presence byte is `0x01`, exactly one value of the schema-defined inner
type follows.

The inner type is part of the schema. It is not inferred from bytes.

### 4.9 Lists

Lists are encoded as:

```text
u32_count || element_0 || element_1 || ... || element_n
```

The schema defines maximum count.

List order is consensus-relevant.

Two lists with the same elements in different order are different consensus
values and produce different HNCS bytes.

### 4.10 Sets

Sets are encoded as lists sorted by bytewise ascending canonical element
encoding.

Duplicates are invalid.

Set semantic order is not consensus-relevant, but encoded order is mandatory.

Rules:

- each element is first encoded using its own schema-defined canonical HNCS
  encoding
- encoded elements are sorted by bytewise ascending order
- the set is encoded as `u32_count || sorted_element_0 || ... || sorted_element_n`
- duplicate canonical element encodings are invalid
- decoders must reject unsorted set encodings
- ordering must not use locale, display text, host comparison functions, hashes,
  or source-language container iteration order

### 4.11 Maps

Maps are encoded as:

```text
u32_count || key_0 || value_0 || key_1 || value_1 || ... || key_n || value_n
```

Entries are sorted by bytewise ascending canonical key encoding.

Duplicate keys are invalid.

The schema defines maximum count.

Map semantic insertion order is not consensus-relevant, but encoded order is
mandatory.

Rules:

- each key is first encoded using its schema-defined canonical HNCS encoding
- entries are sorted by bytewise ascending canonical key encoding
- values are not used for ordering
- the map is encoded as
  `u32_count || sorted_key_0 || value_0 || ... || sorted_key_n || value_n`
- duplicate canonical key encodings are invalid
- decoders must reject unsorted map encodings
- ordering must not use locale, display text, host comparison functions, hashes,
  or source-language container iteration order

For example, `map<string, u8>` is sorted by encoded string bytes, including the
HNCS length prefix, not by human lexical order.

### 4.12 Compound Type Parameters

HNCS compound types are schema-parameterized.

The initial HNCS v0.1 compound profile defines conformance vectors only for:

- `optional<T>` where `T` is an HNCS primitive type
- `list<T>` where `T` is an HNCS primitive type
- `set<T>` where `T` is an HNCS primitive type
- `map<K, V>` where `K` and `V` are HNCS primitive types

Nested compound types require an explicit schema decision before becoming part
of consensus-critical objects.

Examples requiring additional specification before use:

- `set<set<u64>>`
- `map<bytes, list<string>>`
- deeply nested or recursively defined compound structures

When nested compound types are accepted by a future profile, every nested
collection must have explicit bounds, canonical ordering rules, duplicate
rejection rules, and conformance vectors.

## 5. Object Versioning

Every top-level consensus object includes an explicit object version.

The object specification defines:

- version field width
- version compatibility
- allowed migrations
- unknown version behavior
- hashing behavior
- signing behavior

## 6. Extension Encoding

Extension payloads are encoded as versioned, length-delimited records.

Conceptual structure:

```text
ExtensionRecord
  extension_id
  extension_version
  criticality
  payload_length
  payload
```

Rules:

- extension IDs are unique within their registry
- critical unknown extensions are invalid
- non-critical unknown extensions require explicit preservation rules
- payload length is bounded by schema
- payload bytes are included in hashing only as defined by the owning object
  specification

## 7. Decoder Requirements

HNCS decoders must:

- reject non-canonical boolean values
- reject integer width mismatch
- reject invalid UTF-8 strings
- reject collection sizes above schema limits
- reject duplicate set elements
- reject duplicate map keys
- reject unsorted set or map encodings
- reject unknown enum discriminants unless allowed by schema
- reject trailing bytes
- reject unsupported object versions
- enforce resource limits before allocation where possible

## 8. Module Boundaries

```text
Protocol Schema
      |
      v
HNCS Encoder / Decoder
      |
      +----> Signing Payloads
      +----> Hash Inputs
      +----> Storage Records
      +----> State Tree Keys / Values
      +----> Block and Transaction Bytes
```

Boundary rules:

- HNCS defines bytes, not business semantics.
- Protocol object specifications define field meaning.
- Hashing specifications define digest functions and domain separation.
- RPC specifications define external JSON/gRPC views.
- Storage specifications define physical persistence layout.

## 9. Security Requirements

- HNCS must not accept multiple encodings for the same consensus value.
- HNCS must not depend on reflection order from a programming language.
- HNCS must not depend on local memory layout.
- HNCS must not serialize pointers or object references.
- HNCS must not include unbounded collections.
- HNCS must not silently discard unknown critical data.
- HNCS test vectors are mandatory before implementation.

## 10. Open Architecture Decisions

- final byte order confirmation
- schema definition language
- extension identifier width
- enum discriminant width defaults
- maximum collection size defaults
- canonical error taxonomy
- test vector format
- code generation strategy
