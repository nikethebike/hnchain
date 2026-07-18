# HNChain Core Specification: Genesis

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## 1. Scope

This document specifies the conceptual HNChain genesis block and genesis
manifest model.

It defines the purpose of the genesis message, the genesis manifest, document
commitments, and security requirements.

This document does not define the final block header format, final hash
algorithm, final serialization encoding, genesis allocation, validator set, or
state tree structure.

This specification is constrained by:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`

## 2. Design Goals

- Make genesis data explicit and verifiable.
- Include a neutral genesis message without advertising claims.
- Commit to the initial published documentation set.
- Make the genesis block a reproducible starting point.
- Avoid hidden initialization behavior.

## 3. Genesis Header

Conceptual structure:

```text
GenesisHeader
  version
  timestamp
  chain_id
  genesis_hash
  initial_state_root
  genesis_manifest_hash
  genesis_message
```

Field meanings:

- `version`: genesis header format version.
- `timestamp`: fixed protocol timestamp chosen before genesis generation.
- `chain_id`: canonical chain identifier.
- `genesis_hash`: hash of canonical genesis data as defined by the block format.
- `initial_state_root`: state root after applying genesis state.
- `genesis_manifest_hash`: commitment to the genesis manifest.
- `genesis_message`: bounded UTF-8 message included in genesis commitment.

The exact encoding is defined by HNCS.

The exact hash profile is defined by hash algorithm specifications.

## 4. Genesis Message

The genesis message is a bounded UTF-8 string.

Recommended maximum size:

```text
512 bytes
```

The message is included in the canonical genesis commitment.

The message should be neutral, durable, and non-promotional.

Candidate messages:

```text
HNChain Genesis - An open protocol built for long-term trust, transparency and interoperability.
```

```text
Protocol over platform. Specification before implementation.
```

```text
HNChain Genesis Block - Version 1.0
```

The final message must be selected before genesis generation and must not be
changed after the genesis block is published.

## 5. Genesis Manifest

The genesis manifest records the formal starting context of the chain.

Conceptual structure:

```text
GenesisManifest
  manifest_version
  chain_name
  protocol_version
  genesis_time
  chain_id
  whitepaper_hash
  specification_hash
  genesis_state_hash
  license
  genesis_message
```

Optional future fields:

- git tag
- release artifact hashes
- client compatibility suite hash
- genesis validator set hash
- governance constitution hash

The manifest hash is included in the genesis block.

## 6. Document Commitments

The genesis manifest may commit to:

- whitepaper hash
- protocol specification hash
- ADR set hash
- HN Constitution hash
- compatibility test suite hash
- genesis release tag hash

Document hashes must be computed over canonical file bytes or a documented
archive format.

The document commitment process must define:

- included files
- ordering
- line ending normalization policy, if any
- archive format, if used
- hash profile
- publication location

Without these rules, document hashes are not reproducible.

## 7. Timestamp Role

The genesis timestamp provides historical context and ordering.

It is not a source of randomness.

It is not local node time.

It is a fixed field inside genesis data.

If an external news headline, publication hash, or release tag is included, it
must be documented in the genesis manifest.

## 8. Security Requirements

- Genesis data must have canonical serialization.
- Genesis message length must be bounded.
- Genesis manifest hash must be reproducible.
- Document commitments must define exact input bytes.
- Genesis state must produce the documented initial state root.
- Nodes must reject genesis data that does not match the configured chain ID and
  genesis hash.
- Genesis should not include political, promotional, or short-lived claims.

## 9. Open Architecture Decisions

- final genesis message
- final genesis manifest fields
- final genesis timestamp
- final chain ID format
- final document commitment procedure
- final genesis state format
- final genesis hash profile
- final initial validator set commitment
