# ADR-0041: Genesis Document Commitments Via `extra_data`

Status: Accepted

Date: 2026-09-26

Version: 0.1.0

Depends On:

- ADR-0008: Block Format
- ADR-0038: Genesis Format And Node Daemon Bootstrap

Supersedes: None

Referenced By: None

## Context

Two previously-independent gaps, each already named explicitly in its
own ADR:

- `docs/specs/core/genesis.md` §6 ("Document Commitments") describes
  the genesis manifest committing to a whitepaper hash, protocol
  specification hash, ADR set hash, HN Constitution hash, compatibility
  test suite hash, and genesis release tag hash — but ADR-0038's own
  "Explicitly Not Resolved" states plainly: "`GenesisManifest` has no
  document-commitment fields at all this pass, not placeholder-`None`
  ones." The document-commitment *procedure* itself (which files, what
  order, line-ending normalization, archive format) remains genuinely
  open — genesis.md's own §6 lists every one of those sub-questions
  without answering them.
- `BlockBody.extra_data` (ADR-0008, "Decided: Extra Data Format") is
  already a real, decided mechanism: bounded (`MAX_EXTRA_DATA_LEN =
  256` bytes), opaque, "used only for explicitly specified data" — but
  ADR-0008's own text notes "no content is specified for it anywhere in
  this project yet."

Genesis's own document commitments are exactly the kind of
"explicitly specified data" `extra_data` was built to carry, and
`extra_data` is exactly the bounded-opaque-bytes shape genesis's own
document-commitment framing already assumes ("Document hashes must be
computed over canonical file bytes," genesis.md §6). Connecting them
lets both gaps share one already-decided wire mechanism instead of
genesis inventing a second, parallel one (a `whitepaper_hash`/
`specification_hash`/... field-per-document scheme, as genesis.md's own
original conceptual sketch listed) — this ADR makes that connection
explicit; it does not resolve the document-commitment procedure itself,
which stays exactly as open as before.

## Decision

### Decided: `GenesisManifest.extra_data`

`hn_node::GenesisManifest` (ADR-0038) gains a new field, `extra_data:
Vec<u8>` — bounded by the exact same `hn_state::MAX_EXTRA_DATA_LEN`
constant `BlockBody.extra_data` already uses, not a second, genesis-
specific bound. `GenesisManifest::extra_data_hash(&self)` calls the
exact same `hn_state::extra_data_hash` function a real block would,
not a parallel, independently-defined genesis hash — the whole point
of this decision is that "genesis's document commitments" and "a real
block's `extra_data`" are the same mechanism, reused, not two
coincidentally-similar ones.

In the JSON source file, `extra_data` is an **optional** hex-string
field: absent means "explicitly nothing committed yet" (the case for
every genesis file this codebase currently ships, since no document-
commitment procedure exists), not a hidden non-empty default — genesis
.md's own Design Goal ("Avoid hidden initialization behavior") is
satisfied either way, since an absent field and an explicit empty
string commit to the identical bytes. The devnet example genesis
(`hn-node/genesis/devnet.json`) and the shared test fixture
(`hn-node/tests/support/mod.rs`) both write it explicitly as `""`
anyway, matching genesis.md's own "make genesis data explicit"\goal
more literally than relying on the optional-field default.

`extra_data` is encoded as the manifest's own last field (`write_bytes`,
the same `u32_length || bytes` convention every other bounded-bytes
field in this codebase uses), so it participates in `genesis_hash`
exactly like every other field already does — a genesis file's
document commitments, once real ones exist, are as tamper-evident as
any other genesis content, with no separate commitment step needed.

### Explicitly Not Resolved: The Document-Commitment Procedure Itself

This ADR does not decide which files get hashed, in what order, with
what normalization, or in what archive format (genesis.md §6's own
still-open sub-questions) — it decides only *where the resulting bytes
would go* once those questions are answered. A future pass deciding the
real procedure would produce some canonical byte string (concatenated
digests, a small versioned structure, whatever it decides) and set
`GenesisManifest.extra_data` to it; nothing about *that* decision is
constrained by this one beyond fitting in `MAX_EXTRA_DATA_LEN` bytes —
six 32-byte digests (the maximum genesis.md §6 currently lists) already
fit comfortably inside 256 bytes with room to spare for a small
version/count prefix, so this is not expected to be a real constraint
in practice.

## Rejected Options

### A Field Per Document Commitment (`whitepaper_hash`, `specification_hash`, ...)

Rejected: genesis.md's own original conceptual sketch (§5) listed
these as separate fields, but that would be a second, parallel bounded-
content mechanism duplicating what `extra_data` already is, for no
real benefit — nothing about genesis's own document commitments needs
independent per-field hashing rather than one bounded blob covering all
of them together (they are always published/verified as a set, not
individually).

### Deferring This Connection Until The Document-Commitment Procedure Is Decided

Rejected: the wire-level question ("where do these bytes go") is fully
answerable now, independent of the still-open procedural question
("which bytes"), and answering it now means `GenesisManifest`'s own
schema does not need another breaking change later just to add a field
that was already obviously going to be `extra_data`-shaped.

## Compatibility

Adds one field to `GenesisManifest`'s own HNCS encoding — a breaking
change to `genesis_hash`'s own computation, but this project is
pre-mainnet with no deployed genesis file whose hash needs preserving;
every currently-committed genesis file (`hn-node/genesis/devnet.json`)
is regenerated alongside this ADR.

## Open Decisions

- the document-commitment procedure itself (genesis.md §6, unchanged)
- real genesis validator selection and real allocation-account custody
  (unchanged, ADR-0038)

## Related Specifications

- `docs/specs/core/genesis.md`
