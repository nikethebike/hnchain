# ADR-0008: Block Format

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0003: Address Format
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0022: Protocol Versioning

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

**Decided: initial versions.** `block_version`, `header_version`, and
`body_version` are each `u16`, matching this project's Structure
Version convention. All three start at `1` for the initial profile.
They are independent fields, not one shared value (ADR-0022's "Nested
Structure Versions" rule: nesting is decided by the owning field's own
introduction text, not assumed) — `BlockHeader` and `BlockBody` can
each evolve their own shape without forcing `BlockEnvelope` or each
other to bump in lockstep.

### Header Hash

The block hash is computed from canonical HNCS bytes of `BlockHeader` using a
block header hash profile.

The block hash must not include non-canonical RPC fields, gossip metadata,
database identifiers, or local validation annotations.

**Decided: block hash mechanism.**

```text
block_hash = HASH_PROFILE_0x0001("hnchain.block.header.v1", HNCS(BlockHeader))
```

Reuses `HASH_PROFILE_0x0001` (ADR-0005) with the domain tag already
reserved for this purpose — the same pattern every other hash decided
so far in this project follows (state tree, transaction ID, signing
payload); no second hash profile. `hnchain.block.id.v1`, ADR-0005's
other block-scoped reserved tag, is **not** used here and stays
unassigned: nothing decided so far needs a second block-scoped digest
(the transaction-format precedent needed two — `tx_id` over the full
envelope, a separate signing digest over a payload subset — because a
signature must not cover itself; no analogous split is needed for
`block_hash` yet, since `justification` is explicitly excluded from the
header hash by construction, not via a second digest). Leave it
reserved for whatever future purpose needs it (for example a
proposer-signing digest, if block proposal signing turns out to need
one distinct from `block_hash` itself) rather than assigning it a
meaning now to force a use.

### Parent Link

Every non-genesis block references exactly one `parent_block_hash`.

The parent hash links the block to a unique canonical parent header under the
active hash profile.

### Chain ID

`chain_id`'s format (`uint8`, a small closed registry grown only through
explicit governance action) is decided in ADR-0006 (Transaction Format,
"Chain And Network Binding"), not here: ADR-0000's Required ADR Dependency
Order places ADR-0006 before ADR-0008, so a value `BlockHeader` and
`TransactionEnvelope` both need must be decided in the earlier-numbered
document, not redecided (and potentially redefined inconsistently) in
each place it is consumed. `chain_id` appears here in `BlockHeader` for
the same replay-protection-across-a-lineage-split reason it appears in
`TransactionEnvelope`.

### Height, Round, And Epoch

`height` identifies the block position in the chain.

`round` identifies consensus retry or voting round semantics when the selected
consensus protocol uses rounds.

`epoch` identifies validator set periods when the selected consensus
protocol uses them (for example, validator set rotation). This is a
**consensus-protocol concept**, owned by the future consensus
specification — distinct from `protocol_epoch` below, which this ADR
owns and which exists regardless of which consensus protocol HNChain
selects.

The exact semantics of `epoch` are defined by consensus specifications.

### Protocol Epoch

**Decided, closing a real gap**: `BlockHeader` gets a dedicated
`protocol_epoch` field, `u64`, separate from `epoch` above.

ADR-0022 (Protocol Versioning, Accepted) mandates this exact field:
"recorded at the block level; the exact field, width, and encoding are
owned by ADR-0008" — and explicitly warns it "should be present in the
block header from genesis... rather than retrofitted," since adding it
later is a breaking header change. ADR-0008's `BlockHeader` never
actually added it: the pre-existing `epoch` field's old description
("validator set *and protocol-parameter* periods") read close enough
to ADR-0022's `protocol_epoch` to obscure that they are different
concepts with different owners and different purposes — `epoch` is a
consensus-protocol detail that may not even apply to every consensus
protocol HNChain could select; `protocol_epoch` is mandatory,
consensus-protocol-independent, and exists specifically so a node can
tell "which set of ADR-defined structure-version profiles is active at
this height" without reference to any particular consensus mechanism.
Reinterpreting the existing `epoch` field to mean `protocol_epoch`
instead of adding a new one was considered and rejected: ADR-0022
explicitly needs `protocol_epoch` to exist independently of whatever a
consensus protocol's own epoch concept turns out to be (a chain running
a round-based, epoch-free consensus protocol would still need
`protocol_epoch`, but would have no use for the consensus `epoch`
field at all) — collapsing them into one field would silently
reintroduce the coupling ADR-0022 was written to prevent.

`protocol_epoch` is `u64`, matching `height`'s width convention
(monotonically increasing, network-scoped, per ADR-0022). It is not
nested under any other structure version (ADR-0022, "Nested Structure
Versions": nesting must be stated by the owning document, and this is
the owning document stating the opposite — `protocol_epoch` is
independent by construction, since its entire purpose is to name a set
of otherwise-independent structure versions, not to be one itself).
Governance and activation rules for advancing it are explicitly owned
by a future governance ADR (ADR-0022, Deferred Decisions), not decided
here — this ADR only fixes the field's existence, position, and width.

### Proposer

`proposer` identifies the validator or system authority that proposed the block.

The proposer field must use canonical identity data, not display names or RPC
strings.

**Decided: `proposer` field shape.** `bytes32`, an `address_body`
(ADR-0003) — same shape as `sender`/`recipient` in `TransactionEnvelope`
(ADR-0006, "Sender"), for the same reason: everything needed to
interpret it (which namespace, which network) is available from
context (`network_id` here in `BlockHeader`) or is a currently-fixed
value, so a full `AddressPayload` would be redundant. Usually a
`validator` namespace address (ADR-0003, `validator_address_body`), but
the field itself does not encode which namespace produced it —
`address_body` is namespace-opaque by construction (ADR-0003), so a
genesis or other system-authority-proposed block can use a `protocol`
namespace address (`protocol_address_body`) in the same field without
a format change.

### Timestamp

Block timestamp semantics must be consensus-defined.

A node must not use local wall-clock time to decide state transition results
unless the consensus specification explicitly defines the rule and validation
window.

### Ordered List Commitment (`hn-list-merkle-v1`)

**Decided: a new, dedicated tree profile for ordered lists**, shared by
`transactions_root` and `receipts_root` below (and available to
`events_root` once an event schema exists) — not a reuse of
`hn-smt-256-v1` (ADR-0007). ADR-0007's tree is a *sparse* structure
fixed at exactly 256 levels, built for a huge, mostly-empty key space
(global account state); a block's transaction list is the opposite
shape — small, dense, and sequential (index `0..N-1`, no gaps).
Recomputing a fixed 256-level sparse tree every block to commit to a
few thousand dense items would be real wasted work, not a style
choice — a dense list needs only `ceil(log2(N))` levels. Asked the
user before taking this on, since designing a new tree profile is
comparable in scope to what `hn-smt-256-v1` itself required.

```text
LIST_TREE_PROFILE_ID = 0x0001   (hn-list-merkle-v1, independent
                                  registry from ADR-0007's TREE_PROFILE_ID)
```

Leaves are the ordered list's items' own already-decided canonical
digests — not a separate wrapper hash. Reusing an existing
domain-separated digest as a leaf, rather than inventing a redundant
"list leaf" hash on top of it, is the same principle already applied to
`AccessListV1` entries reusing `state_key` directly (ADR-0006, "Access
List"):

- `transactions_root` leaves: each transaction's `tx_id` (ADR-0006,
  "Transaction ID": `HASH_PROFILE_0x0001("hnchain.transaction.id.v1",
  HNCS(TransactionEnvelope))`).
- `receipts_root` leaves: each receipt's digest,
  `HASH_PROFILE_0x0001("hnchain.receipt.v1", HNCS(ReceiptV1))` — this
  is what `hnchain.receipt.v1` (already reserved in ADR-0005's
  conceptual domain tag list) is for.

Internal node combination is one shared formula, domain-separated from
every leaf digest (which all come from other, distinct domain tags) and
requiring two new domain tags (added to ADR-0005's conceptual registry
by this decision): `hnchain.list.node.v1` and `hnchain.list.empty.v1`.

```text
node_hash(left, right) =
  HASH_PROFILE_0x0001("hnchain.list.node.v1", u16 LIST_TREE_PROFILE_ID || left || right)

empty_root =
  HASH_PROFILE_0x0001("hnchain.list.empty.v1", u16 LIST_TREE_PROFILE_ID)
```

Tree construction for an ordered list `D` of `n` items (`MTH`, "Merkle
Tree Hash"), following RFC 6962 (Certificate Transparency) exactly —
not an ad hoc scheme:

```text
MTH(D[0:0])   = empty_root                                    (n = 0)
MTH(D[0:1])   = D[0]                                           (n = 1, the
                                                                 leaf digest
                                                                 itself)
MTH(D[0:n])   = node_hash(MTH(D[0:k]), MTH(D[k:n]))            (n > 1)
                where k is the largest power of two < n
```

The `n = 1` case uses the leaf digest directly with no extra hashing
step, unlike RFC 6962's own construction (which re-hashes even a single
leaf with a type-prefix byte to keep leaf and root values
distinguishable). RFC 6962 needed that prefix because it has no other
domain separation; this project already gets the equivalent guarantee
for free from `tx_id`/`hnchain.receipt.v1` being domain-separated from
`hnchain.list.node.v1` at the hash-profile level — a leaf digest and an
internal node digest can never collide in meaning regardless of tree
position, so no extra wrapping is needed.

The largest-power-of-two split (rather than duplicating an unpaired
leaf, as pre-2012-fix Bitcoin did) is deliberate: duplicating an
unpaired leaf to force a balanced pairing is exactly the construction
behind CVE-2012-2459, where a block containing a duplicated transaction
could produce the same Merkle root as a differently-shaped block. The
RFC 6962 split makes tree shape a pure function of `n`, with no
pairing ambiguity to exploit.

### Transactions Root

`transactions_root` commits to the ordered canonical transaction list included
in the block: `MTH` (`hn-list-merkle-v1`, above) over each transaction's
`tx_id`, in block order.

Transaction order is consensus-relevant.

Changing transaction order changes the block.

### State Root

`state_root` is the state commitment after applying the block's ordered
transactions to the parent state.

The state root is defined by ADR-0007 and the accepted state tree specification.

### Receipts Root

`receipts_root` commits to deterministic execution receipts: `MTH`
(`hn-list-merkle-v1`, above) over each receipt's
`HASH_PROFILE_0x0001("hnchain.receipt.v1", HNCS(ReceiptV1))` digest,
in the same order as `transactions_root`.

A minimal `ReceiptV1` core shape (`receipt_version`, `tx_id`, `status`)
is decided in ADR-0006 ("Receipts"), not here — the same
earlier-document-decides, later-document-consumes direction already
used for `chain_id`. `fee_charged`, `resource_usage`, and
`emitted_event_references` remain open until the fee, event, and HNVM
specifications are accepted; `receipts_root`'s own commitment structure
is now decided (above) independently of that remaining content.

### Events Root

`events_root` commits to consensus-visible events produced by transaction
execution.

Events intended only for local indexing must not be confused with
consensus-visible events. No event schema is decided yet (ADR-0006,
"Events") — gated on HNVM.

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

### Block Size And Transaction Count Limits

**Decided: `MAX_BLOCK_SIZE = 8388608` bytes (8 MiB) and
`MAX_TRANSACTIONS_PER_BLOCK = 10000`**, both checked on the raw encoded
block as an early validation stage, before expensive per-transaction
work. Implementation DoS bounds, not derived, same class as ADR-0006's
`MAX_TRANSACTION_SIZE` — picked for generous headroom, not a formula.

The two are independent bounds, not one derived from the other: byte
size alone does not bound per-block processing cost, since a block
packed with many small transactions can carry far more signature
verifications and state-tree writes than a block near the byte limit
but built from few large transactions. `MAX_TRANSACTIONS_PER_BLOCK`
bounds that cost directly regardless of how it is distributed across
`MAX_BLOCK_SIZE`. Raise either bound later if real usage needs it;
never lower it silently once blocks exist on any live network.

Both are fixed protocol constants for this profile, not (yet) a live
`protocol_parameters_hash`-committed adjustable parameter — whether a
future profile makes size limits governance-adjustable is a question
for "protocol parameter commitment format" (still open), not assumed
here, matching how ADR-0006 treats `MAX_TRANSACTION_SIZE`.

## Validation Pipeline

Conceptual validation flow:

```text
block_bytes
  -> size limits
  -> HNCS decode
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

- final header field registry (sum of the fields below; not a
  standalone decision)
- final body section registry (sum of the same)
- events root format (commitment mechanism — `hn-list-merkle-v1` — is
  decided; content stays open, gated on an event schema, itself gated
  on HNVM)
- consensus root format — blocked on a consensus protocol being
  selected (`docs/rfc/consensus/consensus-architecture.md` is
  Proposed, not Accepted)
- evidence root format — blocked on the same consensus track
  (`docs/rfc/consensus/slashing-and-accountability.md`)
- finality justification format — blocked on the same consensus track
  (`docs/rfc/consensus/finality-rules.md`,
  `vote-messages-and-quorum-certificates.md`)
- timestamp validation window — blocked on the same consensus track
- epoch transition rules — blocked on the same consensus track (the
  consensus-protocol `epoch` field above, not `protocol_epoch`, which
  is already decided)
- protocol parameter commitment format — blocked on a governance model
  existing: unlike `fee_limit`/`ReceiptV1`, there is currently no
  partial structure to decide (no adjustable parameter has been named
  anywhere yet), not just an incomplete one
- genesis block compatibility rules — blocked on
  `docs/specs/core/genesis.md` reaching Accepted (currently Draft)

## Related Specifications

- `docs/specs/core/block-format.md`
- `docs/rfc/consensus/consensus-architecture.md`
