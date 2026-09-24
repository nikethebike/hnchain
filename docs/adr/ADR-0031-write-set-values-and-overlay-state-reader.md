# ADR-0031: Write-Set Value Bytes And Overlay State Reader

Status: Proposed

Date: 2026-09-24

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0019: Storage / State Interfaces
- ADR-0030: Transaction And Block Application (V1 Scope)

Supersedes: None

Referenced By:

- ADR-0032: Governance Chamber-Weight Query And Propose/Vote Wiring

## Context

ADR-0030's own "Explicitly Not Resolved" section named a real gap and
explained exactly why it wasn't fixed on the spot: `apply_block` applies
every transaction in a block against the *same* pre-block `StateReader`.
A second transaction from a sender whose nonce or balance an *earlier*
transaction in the same block already changed does not see that change
— both are checked against stale, pre-block state. Concretely: sender
`S` submits two transactions in one block, both carrying `S`'s current
on-chain nonce `N` (correct for the first, stale for the second); both
pass `apply_transaction`'s nonce check unless a caller does something
external to prevent it, and the second's write-set nonce leaf would
overwrite the first's rather than advance past it — a real
double-inclusion / stale-balance hazard for any caller that hands
`apply_block` a set of transactions without already deduplicating
same-sender nonces itself, which is precisely the burden the write-set
producer should not have to carry.

The fix ADR-0030 identified but declined to build there: an
overlay/staging `StateReader` that layers "this block's writes so far"
on top of the base reader, consulted by every subsequent transaction's
own `fetch_*` calls. Building it requires every `apply_*` function in
this crate to expose real canonical value bytes for what it writes, not
just `(state_key, leaf_hash)` pairs — `tree::Leaf` was built for
[`compute_state_root`], which only ever needs hashes, never the
original bytes back. No existing `apply_*` function's return value can
today answer a `StateReader::get` call, because `StateReader::get`'s own
contract (ADR-0019, "Canonical Bytes At Boundaries") is to return the
object's canonical encoding, not a hash.

This ADR closes that gap: every `apply_*` function's return type changes
from `Leaf`/`[Leaf; N]`/`Vec<Leaf>` to a new `Write`/`[Write; N]`/
`Vec<Write>` shape carrying real value bytes, and a new `OverlayReader`
composes a base `StateReader` with a block's accumulated writes so far.

## Decision

**`Write { state_key: Digest, value: Vec<u8> }`**, a new struct in
`state_store.rs` (next to `StateReader`/`StateWriter`, since its
`value` field is exactly the shape both traits already read/write —
this is the pairing `StateWriter::set(state_key, value)` takes as two
arguments, bundled once so it can travel as a single return value
through every `apply_*` function and into `OverlayReader`). Derives
`Clone`/`Debug`/`Eq`/`PartialEq`, mirroring `Leaf`'s own traits.

**`leaf_for_write(write: &Write) -> StateResult<Leaf>`**, a new pure
function in `tree.rs` (next to `Leaf`/`compute_state_root`, the one
place that still needs `Leaf`-shaped hash pairs): `leaf_hash(&write.state_key,
&value_hash(&write.value)?)?`. This is the one place the
`value_hash`/`leaf_hash` computation ADR-0007 defines now happens —
every `*_leaf` helper this crate had (`balance_leaf`, `asset_leaf`,
`nonce_leaf`, `identity_value_leaf`, `permission_value_leaf`,
`validator_record_leaf`) is renamed to its `*_write` equivalent and
stops computing a hash at all, just building `Write { state_key, value:
value.encode()... }` directly — simpler than before, not just
relocated. `Leaf` itself, and `compute_state_root`, are unchanged:
state-root computation is still defined over a complete `Leaf` set
(ADR-0007), still out of scope for a block's write-set delta, exactly
as ADR-0030 already decided — this ADR only makes it *possible* to
derive that `Leaf` set later, from a store built by folding many
blocks' `Write`s, once a real backend exists (ADR-0019's still-open
item).

**Every `apply_*`/`apply_*_with_receipt` function's return type changes
from `Leaf` to `Write`, in the same shape it already had** (a fixed-size
array stays a fixed-size array, `Vec` stays `Vec`, `Option` stays
`Option`) — this is a mechanical rename plus a dropped hash computation,
not a new design question, in every one of: `transfer.rs`
(`apply_transfer`/`apply_transfer_with_receipt`, `[Write; 2]`),
`nonce_transition.rs` (`nonce_write`, was `nonce_leaf`),
`identity_transition.rs` (`apply_identity_bootstrap`/
`apply_identity_rotation`, `Write`), `permission_transition.rs`
(`apply_permission_update`/`apply_permission_update_with_receipt`,
`Vec<Write>`), `validator_transition.rs` (`apply_stake`/`apply_unstake`/
`apply_validator_update`/`apply_unbonding_release`, `[Write; 1]`/
`Option<[Write; 2]>`), and `block_transition.rs`
(`AppliedTransaction.write_set: Vec<Write>`).

**`governance_transition.rs` is explicitly NOT touched by this ADR.**
`apply_propose`/`apply_vote`/`finalize_proposal` still return
`Leaf`-shaped values. Governance is not dispatched by `apply_transaction`
at all (ADR-0030, "Explicitly Not Resolved": needs a chamber-weight
query this crate does not have yet) and so never participates in
`apply_block`'s overlay reader — converting it now would be unused
work ahead of a real caller, the same "no speculative architecture"
discipline ADR-0019's own trait scope already applied. Whoever wires
governance into `apply_transaction` in the future will need to make the
same `Leaf` → `Write` change there at that time; named here so it is
not a surprise.

**`OverlayReader<'a, R: StateReader>`**, a new `overlay_reader.rs`
module: wraps a `base: &'a R` plus a `pending: BTreeMap<Digest, Vec<u8>>`
of writes accumulated so far. `StateReader::get` checks `pending` first,
falling back to `base.get` on a miss — the standard "staged writes over
a base snapshot" shape, not a novel design. A `fold` method (or
equivalent) accepts a `&[Write]` and inserts each into `pending`,
**later entries overwriting earlier ones for the same `state_key`** —
this is deliberate, not an edge case glossed over: within one
transaction's own write-set this only matters if a single `apply_*`
call ever wrote the same key twice (none currently do), and across
transactions in a block it is exactly the semantics a later transaction
touching the same account must have (its write supersedes an earlier
one in the same block, the same "last write wins within one block"
behavior a real sequential state machine already has). `OverlayReader`
is `pub`: it has exactly one real caller today (`apply_block`, below),
the same "public trait/type, single real implementer so far" precedent
`StateReader`/`StateWriter` themselves already set when only an
in-memory implementation existed.

**`apply_block` is updated to use it.** Instead of applying every
transaction against the same `reader` directly, it builds one
`OverlayReader::new(reader)`, and for each transaction: calls
`apply_transaction(envelope, &overlay, current_height)?`, then folds
the resulting `AppliedTransaction.write_set` into `overlay` before
moving to the next transaction. This is the actual fix: transaction 2
of a block now sees transaction 1's nonce/balance/etc. writes, closing
the "Intra-block same-sender visibility" gap ADR-0030 named. No change
to `apply_transaction`'s own signature or behavior — it already only
ever took `&impl StateReader`, so it works unchanged against an
`OverlayReader` exactly as it did against a plain base reader.

## Explicitly Not Resolved

**`StateWriter` still has no caller.** `Write` now carries exactly what
`StateWriter::set(state_key, value)` needs, but nothing in this
codebase calls `set` — persisting a block's final write-set to a real
backend once one exists (ADR-0019's own still-open "initial storage
backend") is a distinct future step, not attempted here.

**`compute_state_root` still cannot be fed a block's write-set alone.**
Unchanged from ADR-0030's own reasoning: it needs the complete current
leaf set, and nothing in this project maintains that yet. `leaf_for_write`
makes the *conversion* possible (`Write` → `Leaf`) once a real store
that can produce a complete set exists; it does not create that store.

**Governance's own `Leaf`-returning functions are unconverted**, named
above — real follow-up work whenever `governance` gets wired into
`apply_transaction`, not before.

**Cross-block visibility is still out of scope.** `OverlayReader` only
ever wraps one block's worth of writes; nothing here persists a write
set from one block so the next block's `apply_block` call starts from
it — that is exactly the `StateWriter`/durable-backend gap above, not a
new one.

## Rejected Options

### Keeping `Leaf` As The Return Type, Adding `Write` As A Second, Parallel Return Value

Rejected: would make every `apply_*` function return both a hash and
the bytes that hash is supposed to be over, computed independently at
the same call site — a real risk of the two silently drifting apart
(a bug in one but not the other would be invisible, since nothing
would ever cross-check them against each other), and pure redundant
work computing a hash immediately before returning bytes that need it.
This project has already found and fixed the identical "computed value
duplicated instead of derived" mistake four separate times this session
(`TransactionSigningPayload.protocol_name`, `AddressPayload.checksum_profile`,
vote context's `signing_purpose`, `ValidatorSetCommitmentV1.hash_profile`)
— `leaf_for_write` as a pure, on-demand derivation from the one
authoritative `Write` value is the same fix applied a fifth time.

### A Second Set Of `apply_*_values` Functions Alongside The Existing `Leaf`-Returning Ones

Rejected: doubles the API surface of every transition module for no
real benefit — every current caller of the `Leaf`-returning form (test
assertions, `transfer_integration.rs`) can be updated to work from
`Write` directly (deriving a `Leaf` via `leaf_for_write` only where a
test's real point is checking the hash itself), so there is no actual
need for both forms to coexist.

### Fixing Intra-Block Visibility By Deduplicating Same-Sender Transactions Before `apply_block` Runs

Rejected: pushes the correctness burden onto every future caller of
`apply_block` to pre-sort/deduplicate by sender before calling it, an
easy thing to forget and silently get wrong, versus fixing it once,
correctly, inside `apply_block` itself where the information (the
block's own transaction order) already lives.

## Security Considerations

Same-block double-nonce inclusion (the risk ADR-0030 flagged as
unmitigated):

- Risk: two transactions from the same sender in one block, the second
  carrying a nonce only valid after the first is applied.
- Mitigation: now handled correctly — the second transaction's
  `fetch_nonce` call goes through `OverlayReader`, which already
  reflects the first transaction's nonce-write, so a genuinely stale
  nonce is rejected (`StateError::NonceMismatch`) exactly as it would be
  across two separate blocks. This ADR is the mitigation ADR-0030
  deferred, not a new risk.

`OverlayReader`'s `pending` map is unbounded by this ADR:

- Risk: a block with `MAX_TRANSACTIONS_PER_BLOCK` (ADR-0008, 10,000)
  entries, each writing several leaves, accumulates a bounded but
  non-trivial in-memory map for the duration of one `apply_block` call.
- Mitigation: bounded by the same `MAX_TRANSACTIONS_PER_BLOCK`/
  `MAX_BLOCK_SIZE` limits ADR-0008 already enforces upstream of this
  function; no new unbounded-growth surface is introduced.

## Compatibility

Breaking within `hn-state` only (no external crate depends on the
changed signatures yet — `hn-consensus`/`hn-node` are still stubs):
every `apply_*`/`apply_*_with_receipt` function's return type changes
from `Leaf`-shaped to `Write`-shaped, and `AppliedTransaction.write_set`
changes from `Vec<Leaf>` to `Vec<Write>`. No wire-format/encoding change
anywhere — `Write.value` is the exact same canonical bytes each
`*_leaf` helper already computed a hash over, just no longer discarded
after hashing.

## Open Decisions

- `StateWriter` persistence of a block's final write-set (needs a real
  backend, ADR-0019's own still-open item)
- `compute_state_root` integration once a real backend can produce a
  complete leaf set
- governance's own `Leaf` → `Write` conversion, whenever `governance`
  gets wired into `apply_transaction` — resolved, ADR-0032
- cross-block (not just intra-block) write visibility, gated on the
  same real-backend dependency as `StateWriter` above

## Related Specifications

- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0019-storage-state-interfaces.md`
- `docs/adr/ADR-0030-transaction-and-block-application.md`
