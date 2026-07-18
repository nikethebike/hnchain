# ADR-0008: Block Format

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0007: State Tree

Supersedes: None

## Context

Blocks are the canonical containers that bind transaction ordering, execution
results, state roots, consensus metadata, and protocol versioning into a single
verifiable object.

The block format must be stable enough for long-term archival verification and
flexible enough to support consensus evolution, signature algorithm migration,
state tree migration, snapshots, light clients, and future HNVM versions.

HNChain must not let implementation details such as database layout, RPC
encoding, network packet framing, or local mempool ordering define block
validity.

## Decision

HNChain uses a versioned block envelope with a canonical header and canonical
body.

Conceptual structure:

```text
BlockEnvelope
  block_version
  header
  body
  justification
```

Conceptual header:

```text
BlockHeader
  header_version
  chain_id
  network_id
  height
  round
  epoch
  parent_block_hash
  proposer
  timestamp
  transactions_root
  state_root
  receipts_root
  events_root
  consensus_root
  evidence_root
  protocol_parameters_hash
  extra_data_hash
```

Conceptual body:

```text
BlockBody
  body_version
  transactions
  receipts
  evidence
  extra_data
```

The block hash is a domain-separated hash of the canonical block header.

The body is validated by recomputing all committed roots and executing the
ordered transactions against the parent state root.

## Normative Rules

### Versioned Block Envelope

Every block includes `block_version`.

Nodes must not infer block format from byte length, block height, network
message type, client version, or consensus engine implementation.

### Header Hash

The block hash is computed from canonical HNCS bytes of `BlockHeader` using a
block header hash profile.

The block hash must not include non-canonical RPC fields, gossip metadata,
database identifiers, or local validation annotations.

### Parent Link

Every non-genesis block references exactly one `parent_block_hash`.

The parent hash links the block to a unique canonical parent header under the
active hash profile.

### Height, Round, And Epoch

`height` identifies the block position in the chain.

`round` identifies consensus retry or voting round semantics when the selected
consensus protocol uses rounds.

`epoch` identifies validator set and protocol-parameter periods when the
selected consensus protocol uses epochs.

The exact semantics are defined by consensus specifications.

### Proposer

`proposer` identifies the validator or system authority that proposed the block.

The proposer field must use canonical identity data, not display names or RPC
strings.

### Timestamp

Block timestamp semantics must be consensus-defined.

A node must not use local wall-clock time to decide state transition results
unless the consensus specification explicitly defines the rule and validation
window.

### Transactions Root

`transactions_root` commits to the ordered canonical transaction list included
in the block.

Transaction order is consensus-relevant.

Changing transaction order changes the block.

### State Root

`state_root` is the state commitment after applying the block's ordered
transactions to the parent state.

The state root is defined by ADR-0007 and the accepted state tree specification.

### Receipts Root

`receipts_root` commits to deterministic execution receipts.

Receipt format remains open until transaction execution, fee, event, and HNVM
specifications are accepted.

### Events Root

`events_root` commits to consensus-visible events produced by transaction
execution.

Events intended only for local indexing must not be confused with
consensus-visible events.

### Consensus Root

`consensus_root` commits to consensus-specific metadata required to validate
finality, validator set changes, voting data, or other consensus artifacts.

The exact content is defined by the consensus specification.

### Evidence Root

`evidence_root` commits to Byzantine evidence included in the block, such as
double-signing proofs or other slashable behavior if slashing is activated.

The exact evidence schema is defined by validator and consensus specifications.

### Protocol Parameters Hash

`protocol_parameters_hash` commits to the active protocol parameters for the
block.

This prevents ambiguity during upgrades and parameter transitions.

### Extra Data

`extra_data` is bounded and versioned.

It may be used only for explicitly specified data. It must not become an
unbounded escape hatch for consensus behavior.

### Justification

`justification` contains finality proof data or commit certificates required by
the selected consensus protocol.

The block hash excludes `justification` unless the consensus specification
explicitly requires otherwise.

This allows a header hash to identify proposed content while finality data can
be verified as a separate proof over that content.

## Validation Pipeline

Conceptual validation flow:

```text
block_bytes
  -> HNCS decode
  -> size limits
  -> version checks
  -> chain and network checks
  -> parent lookup
  -> header hash verification
  -> body root verification
  -> proposer and consensus checks
  -> transaction validation
  -> deterministic execution
  -> state root verification
  -> receipts and events verification
  -> finality justification verification
```

Cheap checks should run before expensive execution.

## Rejected Options

### Block Hash Over Entire Network Packet

Rejected because gossip envelopes, compression, relay metadata, and transport
framing are not consensus objects.

### Block Format Without Version

Rejected because HNChain must support long-term protocol evolution.

### Transactions As Unordered Set

Rejected because state transitions are order-dependent in an account-based
model.

### Optional State Root

Rejected because every accepted block must commit to post-execution state.

### JSON Block As Consensus Object

Rejected because JSON creates ambiguity in ordering, number representation,
Unicode handling, and canonical hashing.

### Unlimited Extra Data

Rejected because unbounded extra data creates denial-of-service and archival
growth risks.

## Alternatives Considered

### Header Commits Only To Transactions And State

Advantages:

- simpler header
- smaller block metadata

Disadvantages:

- weaker support for light clients, receipts, events, evidence, and upgrade
  verification
- more dependence on trusted full-node RPC responses

### Header Commits To Multiple Independent Roots

Advantages:

- clearer verification boundaries
- better support for light clients and bridges
- allows independent proof systems for transactions, receipts, events, and
  evidence

Disadvantages:

- larger header
- more roots to specify and test
- higher implementation discipline required

### Justification Inside Header Hash

Advantages:

- one hash commits to content and finality proof

Disadvantages:

- may complicate consensus protocols where finality certificates are produced
  after proposal
- can make block identity depend on equivalent certificate encodings

## Security Considerations

Header/body mismatch:

- Risk: a node accepts a body that does not match committed roots.
- Mitigation: recompute all body roots before acceptance.

State transition mismatch:

- Risk: nodes compute different post-state roots.
- Mitigation: deterministic execution, canonical transaction order, and
  ADR-0007 state tree rules.

Consensus proof confusion:

- Risk: finality proof is valid for another block, round, epoch, or chain.
- Mitigation: justification must bind to chain ID, network ID, height, round,
  epoch, block hash, and signing purpose.

Timestamp manipulation:

- Risk: proposer influences execution or validity through local-time ambiguity.
- Mitigation: timestamp semantics are consensus-defined and bounded.

Data availability:

- Risk: a header is propagated without enough body data for validation.
- Mitigation: block propagation and consensus specifications must define body
  availability requirements before finality.

Extra data abuse:

- Risk: unbounded or underspecified extra data becomes a covert protocol layer.
- Mitigation: size limits, versioning, and explicit schemas.

Historical verification breakage:

- Risk: future hash or signature migrations make old blocks unverifiable.
- Mitigation: versioned hash profiles, cryptographic identity profiles, and
  archival verification rules.

## Compatibility

Adding a new header field is a block-version change unless the field is already
reserved and has defined default semantics.

Changing the meaning of an existing field is a major protocol change.

New body sections may be backward-compatible only if:

- the body version supports them
- the corresponding root commitment is defined
- old nodes reject unsupported blocks deterministically before activation
- activation rules are explicit

## Open Decisions

- final header field registry
- final body section registry
- initial block version
- initial header version
- initial body version
- block hash profile
- transactions root format
- receipts root format
- events root format
- consensus root format
- evidence root format
- finality justification format
- block size limits
- transaction count limits
- timestamp validation window
- proposer identity encoding
- epoch transition rules
- protocol parameter commitment format
- genesis block compatibility rules

## Related Specifications

- `docs/specs/core/block-format.md`
