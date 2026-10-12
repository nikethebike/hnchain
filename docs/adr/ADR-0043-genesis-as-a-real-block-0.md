# ADR-0043: Genesis As A Real Block 0

Status: Accepted

Date: 2026-10-12

Version: 0.1.0

Depends On:

- ADR-0008: Block Format
- ADR-0038: Genesis Format And Node Daemon Bootstrap
- ADR-0040: Real Block Hash In `hn-node`
- ADR-0042: BlockEnvelope And Receiver-Side Header Verification

Supersedes: None

Referenced By: None

## Context

A real, repeatedly-named open item, closed here: `docs/specs/core/
block-format.md` §12 says plainly "Genesis is a special block with
fixed, published data... The final block format must define how
genesis maps into `BlockEnvelopeV1`." ADR-0008's own Open Decisions
("genesis block compatibility rules") and ADR-0038's own ("genesis's
eventual integration into a real `BlockHeader`/block 0") both named the
same gap, each time concluding the real type now existed but nothing
yet constructed an actual genesis block from it.

Concretely, before this ADR: `hn-node`'s own `Ctx.last_block_hash`/
`Ctx.last_timestamp` (ADR-0040, ADR-0042) started from `[0; 32]`/`0` —
arbitrary, shared-by-convention sentinels, not values derived from
genesis's own real content. ADR-0042's own receiver-side
`parent_block_hash`/timestamp-monotonicity check is only as meaningful
as what it checks against; checking the very first real block's
`parent_block_hash` against an arbitrary constant every node already
agrees on by convention provides no actual security value — anyone
could trivially satisfy it without even knowing genesis's real
content (the validator set, the three HNCOIN allocations, the genesis
message). Resolving "genesis maps into a real block" turns that first
check into one that actually depends on genesis's real, verified data.

## Decision

### Decided: `genesis_block_header`

A new function in `hn-node` (`node.rs`, alongside `build_block_header`,
the per-round equivalent):

```rust
fn genesis_block_header(manifest: &GenesisManifest) -> NodeResult<BlockHeader>
```

Every node constructs this identically from the same, already
`genesis_hash`-verified `GenesisManifest` (ADR-0038) — not consensus-
voted, not gossiped, not a sentinel. Field mapping:

- `height`: `BlockHeight::GENESIS` (`0`) — see "Decided: Height
  Renumbering," below.
- `round`/`epoch`: `0` — genesis is not reached through any real round.
- `protocol_epoch`: `ProtocolEpoch::GENESIS`, the same value every
  block in this devnet already uses.
- `parent_block_hash`: `[0; 32]` — genesis has no predecessor at all;
  this is the chain's own actual first content, not a block with
  something before it (distinct from every other block's
  `parent_block_hash`, which is now always a real hash, never this
  sentinel — see "Decided: Height Renumbering").
- `proposer`: `[0; 32]` — no validator proposes genesis; `BlockHeader.
  proposer`'s own doc comment already allows "the proposing validator
  OR SYSTEM AUTHORITY's own address_body" (ADR-0008), and `docs/specs/
  core/block-format.md` §5 already anticipates "a genesis or
  system-authority proposer."
- `timestamp`: `manifest.genesis_time` converted from Unix seconds
  (ADR-0038's own unit for that field) to `UnixTimeMillis` (ADR-0008's
  own unit for this one) — a real unit mismatch between two already-
  decided types, converted once here rather than left silent.
- `transactions_root`/`receipts_root`/`evidence_root`/`extra_data_hash`:
  computed from a real `BlockBody` whose `extra_data` is `manifest.
  extra_data.clone()` (ADR-0041) — genesis's own document-commitment
  bytes, whatever they are, actually affect this hash now, not an
  always-empty stand-in.
- `events_root`: `list_empty_root()` — no `events` field exists in
  `BlockBody` at all yet (HNVM-gated), the same invariant every other
  block already relies on.
- `state_root`: `GenesisManifest::initial_state_root()` — already the
  real value `Ctx.state_root` has used since ADR-0040.
- `consensus_root`: the same fixed `DEVNET_VSC` every other block in
  this devnet already uses — not a separately-computed "real" value
  just for genesis, which would make genesis's own `consensus_root`
  inconsistent with the block that follows it.
- `protocol_parameters_hash`: `protocol_parameters_placeholder_hash()`
  — the same placeholder every other block already uses.

### Decided: Height Renumbering

Genesis is height `0`, uniquely. The first real, consensus-produced
block now starts at height `1` (`node::run`'s own `ConsensusEngine::
new_height` call changes from `BlockHeight::GENESIS` to `BlockHeight::
new(1)`) — a genuine behavior change, not merely a new value feeding
an unchanged sequence. This was a real fork, asked of the user rather
than assumed: the alternative (leave the first real block at height 0,
let genesis's own header also claim height 0) would mean two distinct
headers — genesis's and the first real block's — share one height
number even though one is the other's parent, breaking `height =
parent.height + 1` across that one boundary while it holds everywhere
else. The user chose renumbering specifically to keep that invariant
true everywhere, matching ordinary blockchain convention (genesis is
block 0; mining/voting starts at block 1) rather than carving out an
exception.

`Ctx.last_block_hash`/`Ctx.last_timestamp` are now seeded from
`genesis_block_header(&manifest).block_hash()`/`.timestamp` at startup
(`node::run`), not `[0; 32]`/`UnixTimeMillis::from_millis(0)`.
`Ctx.chain_position_verified` stays `true` from startup, unchanged —
genesis's own content is known and agreed by every node before any
real consensus round even begins, so there was never a "might be
wrong" window for the very first real block's parent/timestamp checks
the way there is after a passive catch-up jump (ADR-0039, ADR-0042).

## Rejected Options

### Leaving Height 0 As The First Real Block, Giving Genesis No Height At All

Rejected (the user's own call): avoids renumbering any existing test
or log expectation, but genesis's header would then need either no
`height` field at all (a special case `BlockHeader` does not otherwise
have) or the same `0` the first real block already claims — the latter
breaks `height = parent.height + 1` at exactly the one boundary where
it would matter.

### Computing A Real `consensus_root` For Genesis From Its Own Validator Set

Rejected: `hn_state::consensus_root`/`validator_set_commitment`
already exist as real functions (ADR-0010), but every other block in
this devnet still uses the fixed `DEVNET_VSC` placeholder instead of
calling them — an already-named, separate open item. Computing a real
value just for genesis would make genesis's own `consensus_root`
disagree with the very next block's, which reuses the placeholder;
worse, not better.

## Compatibility

Breaking change: every existing devnet genesis file's effective chain
now starts its real consensus-produced blocks at height 1 instead of
height 0. The two existing multi-process integration tests
(`multi_node_view_change.rs`, `restart_recovery.rs`) were updated for
the new numbering — `multi_node_view_change.rs`'s own "dead round-0
proposer" validator index changed from 0 to 1, since `round_proposer`'s
`(height + round) % len` formula now selects a different validator at
the new starting height (`height=1, round=0` selects index 1, not the
index 0 it selected at `height=0, round=0`). Acceptable pre-mainnet,
consistent with every other breaking devnet change this project has
already made.

## Security Considerations

Closed: the first real block's `parent_block_hash` check (ADR-0042)
was previously checking against an arbitrary, universally-trivially-
satisfiable constant — now it checks against a value only genesis's
own real, agreed content produces, so a node running a different
genesis (different validators, different allocations, a tampered
`extra_data`) would produce a different genesis `block_hash` and
genuinely fail to chain correctly with nodes running the real one.

Not changed: genesis's own content is still trusted as given once
`genesis_hash` matches at startup (ADR-0038's own "Decided: DB Init")
— this ADR does not add new verification of `GenesisManifest` itself,
only makes its already-verified content flow into the first real
block's own parent check.

## Open Decisions

- the document-commitment procedure itself (genesis.md §6, unchanged —
  genesis's own `extra_data` now flows all the way into a real header
  hash, but *which* bytes it should contain remains exactly as open)
- real genesis validator selection and real allocation-account custody
  (organizational, unchanged, ADR-0038)
- a real, non-placeholder `consensus_root` for every block including
  genesis (ADR-0010, unchanged — deliberately not attempted here, see
  "Rejected Options")

## Related Specifications

- `docs/specs/core/block-format.md`
- `docs/specs/core/genesis.md`
