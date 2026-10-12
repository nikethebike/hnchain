# HNChain Core Specification: Block Format

Status: Proposed

Version: 0.3.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0022-protocol-versioning.md`
- `docs/adr/ADR-0042-block-envelope-and-receiver-side-header-verification.md`
- `docs/adr/ADR-0043-genesis-as-a-real-block-0.md`

`docs/adr/ADR-0042-block-envelope-and-receiver-side-header-verification.md`
resolves this document's own long-open `BlockEnvelopeV1` schema (§3, §17)
as a concrete `hn_state::BlockEnvelope` type, and resolves §13's
conceptual validation pipeline into a real, implemented receiver-side
check sequence in `hn-node`. `docs/adr/ADR-0043-genesis-as-a-real-
block-0.md` resolves this document's own §12 ("Genesis Block") and its
own "genesis mapping" Open Decision (§17) — genesis now maps to a real
`BlockHeader`, and the first real consensus-produced block starts at
height 1, not 0. This document keeps its own conceptual framing,
updated in place to point at both ADRs rather than duplicate them.
This remains Proposed, not Accepted: several other sections
(receipt schema, size limits, full genesis-to-block-0 mapping) remain
genuinely open regardless.

## 1. Purpose

This specification defines the conceptual HNChain block format.

A block commits to:

- parent block identity
- ordered transactions
- post-execution state
- deterministic receipts
- consensus-visible events
- consensus metadata
- Byzantine evidence
- active protocol parameters

## 2. Design Boundary

The block format is a consensus object.

Network packets, RPC responses, database records, local indexes, and explorer
views are not block format definitions.

```text
P2P Packet
  -> Canonical Block Bytes
  -> Block Envelope
  -> Header + Body + Justification
  -> Validation
  -> Finality
```

Only canonical block bytes and explicitly referenced consensus proofs can affect
block validity.

## 3. Block Envelope

Conceptual structure:

```text
BlockEnvelopeV1
  block_version
  header
  body
  justification
```

### 3.1 Fields

`block_version`

- Version of the block envelope. `u16`, Structure Version convention.
- Initial value: `1` (ADR-0008, "Versioned Block Envelope").

`header`

- Canonical block header.

`body`

- Canonical block body.

`justification`

- Consensus finality proof or commit certificate.
- Format is defined by consensus specifications.

**Resolved** (ADR-0042): `hn_state::BlockEnvelope` is the concrete
`BlockEnvelopeV1` — `block_version: u16`, `header: BlockHeader`,
`body: BlockBody`, `justification: Option<hn_state::
QuorumCertificate>`. No separate `BlockJustificationV1` wrapper exists:
`QuorumCertificate` (ADR-0012) already carries every field §11, below,
names (`certificate_type`/`height`/`round`/`epoch`/`target_type`/
`target_hash`/`signer_commitment`/`aggregate_proof`), so it fills that
conceptual role directly rather than through a second, parallel type.

## 4. Block Header

Conceptual structure:

```text
BlockHeaderV1
  header_version
  chain_id
  network_id
  height
  round
  epoch
  protocol_epoch
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

All fields are consensus-relevant.

**Resolved**: the HNCS schema is `hn_state::BlockHeader` (ADR-0008,
"Decided: `BlockHeader`/`BlockBody` Structs") — field order and types
exactly as listed above, reusing `hn_core`'s own `BlockHeight`/
`Round`/`Epoch`/`ProtocolEpoch`/`UnixTimeMillis` types rather than bare
integers.

## 5. Header Field Semantics

### 5.1 Header Version

`header_version` identifies the header schema. `u16`, Structure Version
convention, initial value `1` (ADR-0008, "Versioned Block Envelope") —
independent of `block_version`/`body_version`, not nested under them
(ADR-0022's "Nested Structure Versions" rule).

Unknown header versions are rejected unless upgrade rules define acceptance.

### 5.2 Chain ID And Network ID

`chain_id` and `network_id` prevent cross-chain and cross-network confusion.

They must match local node configuration.

`chain_id` (`uint8`) and `network_id` (`uint16`) reuse the same registries
`TransactionEnvelope` uses, decided in ADR-0006 ("Chain And Network
Binding") and ADR-0003 ("Decision 4") respectively — not redecided here,
per ADR-0000's Required ADR Dependency Order (ADR-0006 precedes ADR-0008).

### 5.3 Height

`height` is the block number after genesis.

Genesis height is an open decision and must be specified by the genesis and
block format finalization process.

### 5.4 Round

`round` is a consensus-controlled value.

For consensus protocols without rounds, the field may be fixed by the consensus
profile.

### 5.5 Epoch And Protocol Epoch

`epoch` identifies validator set periods if supported by the consensus
profile — a consensus-protocol concept, owned by the future consensus
specification.

**Decided** (ADR-0008, "Protocol Epoch"): `protocol_epoch` is a
separate `u64` field, not the same as `epoch` above and not owned by
the consensus protocol. It fulfills ADR-0022 (Protocol Versioning,
Accepted)'s mandate for a dedicated, consensus-protocol-independent
field naming which set of ADR-defined structure-version profiles is
active at this height, used only for hard-fork/activation signaling —
a real gap fixed this pass, since `BlockHeaderV1` never previously
carried it (the old, single `epoch` field's "protocol-parameter
periods" wording obscured that these are two different concepts with
two different owners). Governance/activation rules for advancing it
remain owned by a future governance ADR.

### 5.6 Parent Block Hash

`parent_block_hash` identifies the canonical parent header.

**Decided: block hash mechanism** (ADR-0008, "Header Hash"):

```text
block_hash = HASH_PROFILE_0x0001("hnchain.block.header.v1", HNCS(BlockHeader))
```

Reuses `HASH_PROFILE_0x0001` (ADR-0005) with its own reserved domain
tag, matching every other hash decided so far — no second hash
profile. ADR-0005's other block-scoped tag, `hnchain.block.id.v1`,
stays unassigned: nothing decided yet needs a second block-scoped
digest.

Genesis parent semantics are open.

### 5.7 Proposer

`proposer` is the canonical identity of the validator or authority that proposed
the block. `bytes32`, an `address_body` (ADR-0003) — same shape as
`TransactionEnvelope.sender` and for the same reason (ADR-0008,
"Proposer"). Usually a `validator` namespace address, but the field is
namespace-opaque, so a genesis or system-authority proposer can use a
`protocol` namespace address in the same field.

Display names are not consensus values.

### 5.8 Timestamp

`timestamp` is validated only by consensus-defined rules.

It is not a source of randomness and must not expose local clock differences to
state transition logic.

### 5.9 Transactions Root

`transactions_root` commits to the ordered canonical transactions in the body.

**Decided** (ADR-0008, "Ordered List Commitment"): `MTH` (`hn-list-merkle-v1`)
over each transaction's `tx_id` (ADR-0006), in block order. Leaves reuse
`tx_id` directly rather than a separate wrapper hash. See ADR-0008 for
the full tree construction (RFC 6962-style largest-power-of-two split,
deliberately not a duplicate-unpaired-leaf scheme — that construction
is what CVE-2012-2459 exploited).

- transaction leaf encoding: `tx_id` (already a canonical digest)
- ordering rule: block order (positional, matching `transactions_root`)
- empty list root: `HASH_PROFILE_0x0001("hnchain.list.empty.v1", u16 0x0001)`
- tree profile: `hn-list-merkle-v1`, `LIST_TREE_PROFILE_ID = 0x0001`
- hash profile: `HASH_PROFILE_0x0001` (ADR-0005), no new profile
- test vectors: not yet written (implementation task, not a design gap)

### 5.10 State Root

`state_root` is the state commitment after block execution.

It is verified using the state tree specification.

### 5.11 Receipts Root

`receipts_root` commits to deterministic execution receipts.

Receipt format must define:

- transaction index binding
- success and failure semantics
- fee charged
- resource usage
- emitted event references
- state changes summary, if included

`ReceiptV1` (transaction index binding via `tx_id`, success/failure via
`status`) is decided in ADR-0006, "Receipts". Fee charged, resource
usage, and emitted event references remain open, gated on the fee and
event models.

**Decided root construction** (ADR-0008, "Ordered List Commitment"):
`MTH` (`hn-list-merkle-v1`) over each receipt's
`HASH_PROFILE_0x0001("hnchain.receipt.v1", HNCS(ReceiptV1))` digest, in
the same order as `transactions_root`.

### 5.12 Events Root

`events_root` commits to consensus-visible events.

Indexer-only metadata must not be included unless promoted to consensus-visible
event semantics by specification.

No event schema is decided yet (ADR-0006, "Events") — no currently-decided
`tx_type` payload emits one; gated on HNVM. The commitment mechanism
(`hn-list-merkle-v1`, ADR-0008) is already available once one exists —
only the per-event leaf digest formula is missing.

### 5.13 Consensus Root

`consensus_root` commits to consensus-specific metadata.

Examples may include:

- validator set update commitment
- randomness commitment
- vote aggregation metadata
- epoch transition metadata

The exact fields are not defined by this core block format.

### 5.14 Evidence Root

`evidence_root` commits to included Byzantine evidence.

Evidence has no effect unless the consensus and validator specifications define
its validity and consequences.

### 5.15 Protocol Parameters Hash

`protocol_parameters_hash` commits to the active protocol parameter set.

This field supports deterministic validation during upgrades.

### 5.16 Extra Data Hash

`extra_data_hash` commits to bounded, explicitly specified extra data in the
body.

Unspecified extra data is invalid.

**Decided** (ADR-0008, "Decided: Extra Data Format"):
`HASH_PROFILE_0x0001("hnchain.block.extradata.v1", extra_data)` — the
raw `extra_data` bytes hashed directly, not re-encoded, since
`extra_data` is already opaque bytes with no further canonical
structure. `MAX_EXTRA_DATA_LEN = 256` bytes bounds the body-side field
itself (§6). No special-cased "empty" value, unlike
`hn-list-merkle-v1`'s `empty_root`: `extra_data` is one blob, not an
ordered list, so a zero-length value hashes with the same formula as
any other length.

## 6. Block Body

Conceptual structure:

```text
BlockBodyV1
  body_version
  transactions
  receipts
  evidence
  extra_data
```

`body_version` is `u16`, Structure Version convention, initial value `1`
(ADR-0008, "Versioned Block Envelope") — independent of
`block_version`/`header_version`.

The body contains data required to verify header commitments and execute the
block.

**Resolved**: the HNCS schema is `hn_state::BlockBody` (ADR-0008,
"Decided: `BlockHeader`/`BlockBody` Structs") — `transactions: Vec
<TransactionEnvelope>`, `receipts: Vec<ReceiptV1>`, `evidence: Vec
<Vec<u8>>` (opaque, already-canonical bytes; no `ConsensusEvidence`
schema decided yet), `extra_data: Vec<u8>`. `BlockBody::
transactions_root`/`receipts_root`/`evidence_root`/`extra_data_hash`
compute the matching header commitment directly from body content.

**Decided** (ADR-0008, "Decided: Extra Data Format"): `extra_data` is
bounded, opaque `bytes`, `<= MAX_EXTRA_DATA_LEN = 256` bytes. It
carries no separate internal version field of its own — `body_version`
above already covers it, and no content is specified for it anywhere
in this project yet for there to be an interior to version. See §5.16
for `extra_data_hash`'s own commitment formula.

## 7. Transactions

Transactions are stored in canonical block order.

Every transaction must be a canonical transaction envelope.

Validation requires:

- transaction schema validation
- signature verification
- nonce and fee checks
- block inclusion rules
- deterministic execution

Mempool acceptance is not consensus validity.

## 8. Receipts

Receipts are deterministic execution outputs. Core shape (`ReceiptV1`:
`receipt_version`, `tx_id`, `status`) decided in ADR-0006, "Receipts".

Receipt ordering must correspond to transaction ordering.

Receipts must not include:

- local execution timing
- node identifiers
- debug-only traces
- non-canonical error strings
- database-specific details

## 9. Events

Events are deterministic execution outputs intended for protocol-visible
consumption.

Event payloads must be versioned and HNCS-encoded.

Event indexing strategy is outside consensus.

## 10. Evidence

Evidence records consensus-relevant validator misbehavior.

Evidence must bind to:

- chain ID
- network ID
- height
- round or epoch, if applicable
- accused validator identity
- evidence type
- canonical proof bytes

Evidence verification must be bounded.

## 11. Justification

Justification proves finality or consensus acceptance for a block.

Conceptual structure:

```text
BlockJustificationV1
  justification_version
  consensus_profile
  target_block_hash
  target_height
  target_round
  target_epoch
  signatures_or_proof
```

The justification must bind to the block hash and consensus context.

The block hash excludes justification unless the accepted consensus
specification explicitly chooses a different rule.

**Resolved** (ADR-0042): `justification` is `Option<hn_state::
QuorumCertificate>` directly — see §3.1, above. `BlockHash` excludes it
exactly as this section already required: `block_hash` is computed
from `BlockHeader` alone (ADR-0008, "Header Hash"), never from the
envelope's `justification` field.

## 12. Genesis Block

Genesis is a special block with fixed, published data.

The genesis specification defines:

- genesis message
- genesis manifest
- initial state root
- genesis hash
- document commitments
- initial validator set commitment, if present

The final block format must define how genesis maps into `BlockEnvelopeV1` or a
dedicated genesis envelope.

**Resolved** (ADR-0043): genesis maps directly into a real
`BlockHeader` (not a dedicated genesis envelope) — every node
constructs this header identically from the same, already
`genesis_hash`-verified `GenesisManifest`. `height = BlockHeight::
GENESIS` (`0`), `parent_block_hash`/`proposer` are both `[0; 32]`
("no predecessor," "no validator — protocol-authored," per §5's own
`proposer` semantics), `timestamp` is `genesis_time` converted from
seconds to milliseconds, and `state_root` is `GenesisManifest::
initial_state_root()`. The first real consensus-produced block starts
at height `1`, not `0` — genesis alone holds height `0`, so
`height = parent.height + 1` holds across this boundary exactly as it
does between any two real blocks.

## 13. Validation Pipeline

Conceptual validation:

```text
canonical_block_bytes
  -> size limits
  -> HNCS decode
  -> envelope version check
  -> header version check
  -> body version check
  -> chain and network check
  -> parent header lookup
  -> header hash computation
  -> transaction root verification
  -> evidence root verification
  -> extra data root verification
  -> consensus metadata verification
  -> transaction execution
  -> state root verification
  -> receipt root verification
  -> event root verification
  -> justification verification
```

Implementation may reorder independent cheap checks, but final validity must be
equivalent to this pipeline.

**Partially resolved** (ADR-0042): `hn-node`'s own `verify_proposal`
implements the consensus-relevant half of this pipeline for its own
always-empty-block devnet scope — chain/network check, header hash
computation (recomputed and compared against the proposer's own
signature, not a separately-trusted claim), consensus metadata
verification (`proposer` eligibility against `round_proposer`,
`state_root`/`consensus_root`/`protocol_parameters_hash` against this
devnet's own fixed values), and justification verification
(`QuorumCertificate::verify_signatures`). `parent block lookup` is
`parent_block_hash` equality against this node's own last finalized
block, skipped only while this node's own chain position is not yet
known to be correct (mid passive-catchup, ADR-0039). Not resolved:
`transaction execution`/`state root verification` against real,
non-empty transaction content, `receipt root verification` against
real receipts, and `event root verification` against real events —
all still blocked on real transactions existing at all (ADR-0016).
`size limits`/envelope-body-version checks were already resolved by
`BlockEnvelope`/`BlockHeader`/`BlockBody`'s own `decode` methods
(ADR-0008).

## 14. Size Limits

The accepted version must define limits for:

- block byte size
- transaction count
- receipt count
- event count
- evidence count
- extra data size
- justification size
- per-section decode memory
- per-section verification time

**Decided** (ADR-0008, "Block Size And Transaction Count Limits"):
`MAX_BLOCK_SIZE = 8388608` bytes (8 MiB), `MAX_TRANSACTIONS_PER_BLOCK =
10000` — independent bounds (byte size alone does not bound
per-transaction processing cost). Fixed protocol constants for this
profile, matching how ADR-0006's `MAX_TRANSACTION_SIZE` is treated —
not (yet) a live governance-adjustable parameter; raising either is a
future protocol-version decision. Whether size limits become
`protocol_parameters_hash`-committed adjustable parameters in a later
profile is a question for "protocol parameter commitment format"
(still open), not assumed here.

Extra data size — **resolved**: `MAX_EXTRA_DATA_LEN = 256` bytes
(§6, ADR-0008 "Decided: Extra Data Format"). Receipt count, event
count, evidence count, justification size, and per-section decode/
verification budgets remain open, gated on the schemas and
specifications each depends on (event/evidence/justification schemas,
HNVM, consensus protocol).

Limits are consensus parameters and must be committed by
`protocol_parameters_hash` where applicable.

## 15. Security Requirements

Implementations must reject:

- unknown versions
- malformed canonical encodings
- unknown required body sections
- parent hash mismatches
- incorrect committed roots
- duplicate or missing transactions when roots imply otherwise
- invalid transaction order
- invalid state root
- invalid receipts root
- invalid events root
- invalid evidence root
- invalid consensus metadata
- invalid finality justification
- oversized blocks or body sections

Implementations must avoid:

- hashing network packets instead of headers
- trusting RPC-provided block hashes
- trusting receipts without root verification
- trusting events without root verification
- executing transactions before basic block limits are checked

## 16. Test Vectors

The accepted version of this specification must include test vectors for:

- genesis block hash
- empty block
- single transaction block
- multi-transaction block
- transaction ordering change
- invalid transactions root
- invalid state root
- invalid receipt root
- invalid event root
- invalid justification binding
- malformed block rejection
- maximum-size boundary behavior

Test vectors are mandatory before production implementation.

## 17. Open Decisions

- final block envelope schema — **resolved, ADR-0042**:
  `hn_state::BlockEnvelope` (`block_version`/`header`/`body`/
  `justification`)
- final header schema — **resolved**, `hn_state::BlockHeader`
  (ADR-0008, "Decided: `BlockHeader`/`BlockBody` Structs")
- final body schema — **resolved**, `hn_state::BlockBody` (same)
- extra data format — **resolved** (§6/§5.16, ADR-0008 "Decided: Extra
  Data Format")
- genesis mapping — **resolved, ADR-0043**: every node constructs
  genesis's own real `BlockHeader` directly from `docs/specs/core/
  genesis.md`'s own `GenesisManifest` (ADR-0038), using the
  `BlockHeader`/`BlockBody` concrete types (ADR-0008); see §12, above
- receipt schema (`ReceiptV1` core shape decided, ADR-0006 "Receipts" —
  `fee_charged`/`resource_usage`/`emitted_event_references` still open)
- event schema (not decided — gated on HNVM; `hn-list-merkle-v1` root
  construction is ready once one exists, ADR-0008)
- justification schema — **resolved, ADR-0042**: `Option<hn_state::
  QuorumCertificate>` directly, no separate wrapper type (§3.1/§11,
  above)
- timestamp validation semantics — **partially resolved, ADR-0042**:
  `hn-node`'s own receiver-side check requires strict monotonicity
  against the parent block's own timestamp (when this node's own chain
  position is known to be correct); no wall-clock/drift bound exists
  or is decided — deliberately not invented as a new protocol constant
  this pass, still fully open
- evidence schema, consensus metadata schema (beyond the devnet-fixed
  values ADR-0042's own receiver-side check already compares against),
  epoch transition rules — all blocked on `docs/rfc/consensus/*`
  reaching Accepted (currently Proposed)
- protocol parameter schema — still blocked: ADR-0025 (Governance
  Model) decides on-chain signaling votes only, no automatic protocol
  effect; no adjustable parameter has been named anywhere yet to give
  this a partial shape
