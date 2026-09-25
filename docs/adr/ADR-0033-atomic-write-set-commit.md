# ADR-0033: Atomic Write-Set Commit (`StateCommitter`)

Status: Proposed

Date: 2026-09-25

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0007: State Tree
- ADR-0019: Storage / State Interfaces
- ADR-0030: Transaction And Block Application (V1 Scope)
- ADR-0031: Write-Set Value Bytes And Overlay State Reader

Supersedes: None

## Context

`StateWriter::set` (ADR-0019) has existed since `hn-storage` was first
built, but nothing in this codebase has ever called it from real
protocol code — its only caller anywhere is `hn-storage`'s own test
scaffolding, hand-seeding a store with byte values constructed directly
from `ValidatorRecordV1.encode()`, never through `apply_transaction`/
`apply_block`'s own write-set. ADR-0031 gave every `apply_*` function a
real `Write { state_key, value }` output specifically so a write-set
could one day be persisted; nothing has used that capability yet
either.

Naively closing this gap — loop over a `Vec<Write>` calling
`StateWriter::set` once per entry — would work functionally but breaks
a real guarantee ADR-0019 already normatively requires ("Atomic State
Commit": "A node must not expose a partially committed state as valid
chain state"). `RedbStateStore::set`'s own documentation already says
so plainly: "Every read and write goes through its own `redb`
transaction... not a claim that a whole write set commits atomically
together." A loop of individual `set` calls against `RedbStateStore`
today means a failure partway through (disk full, I/O error) leaves
some of a transaction's or a block's writes durably applied and the
rest not — exactly the "partially committed state" ADR-0019 forbids.

ADR-0019's own nine-interface boundary diagram already names the
missing piece: `StateCommitter`, listed distinctly from `StateWriter`.
This ADR defines it, deliberately narrower than ADR-0019's full
"Atomic State Commit" rule (which also lists block header, block body,
receipts, events, and consensus metadata as needing to land in the same
atomic commit) — none of those exist as concrete stored objects in this
codebase yet (no `BlockHeader` type, no receipts/events storage design,
no consensus-metadata storage design), so committing all of them
atomically together is not yet a reachable problem. What *is* reachable
now is exactly what ADR-0031 produced: a deterministic write-set. This
ADR makes that one piece atomic, honestly scoped, not a premature claim
of full block-commit atomicity.

## Decision

**`StateCommitter`**, a new trait in `hn-state::state_store`, alongside
`StateReader`/`StateWriter`/`Write`:

```rust
pub trait StateCommitter {
    fn commit(&mut self, writes: &[Write]) -> StateResult<()>;
}
```

Contract: all-or-nothing. If `commit` returns `Err`, none of `writes`
may be observable afterward via `StateReader::get`; if it returns `Ok`,
every one of them must be. This is a real, backend-specific guarantee a
naive loop of `StateWriter::set` calls does not provide — the entire
reason this is its own trait rather than a default-provided method on
`StateWriter` that would tempt an implementor into believing the naive
loop is good enough. No default implementation is provided, for the
same reason: every implementor must consciously decide how it achieves
atomicity, not silently inherit a non-atomic one.

**`InMemoryStateStore` implements it as a plain loop.** A `BTreeMap`
insert cannot itself fail (already this crate's own established
reasoning for why `StateReader`/`StateWriter` "always return `Ok`" for
this backend) — there is no partial-failure mode for an in-process data
structure to guard against beyond a panic, which would abort the whole
process regardless of how the loop is written. Genuinely atomic in
every practical sense, not just nominally.

**`RedbStateStore` implements it as one real `redb` transaction.**
`begin_write()` once, `table.insert()` once per entry inside it, then
`commit()` once — matching `redb`'s own transactional guarantees
exactly (an uncommitted transaction that errors or is dropped applies
nothing). This is the real fix: where `RedbStateStore::set` opens and
commits a transaction per key, `RedbStateStore::commit` opens exactly
one transaction for the whole `writes` slice.

**`apply_and_commit_block`**, a new composing function in
`hn-state::block_transition`, alongside `apply_block`:

```rust
pub fn apply_and_commit_block<S: StateReader + StateCommitter>(
    transactions: &[TransactionEnvelope],
    store: &mut S,
    current_height: BlockHeight,
    validator_candidates: &[ValidatorRecordV1],
) -> StateResult<BlockApplicationResult>
```

Runs `apply_block` against `store` (reborrowed immutably for that
call), flattens every applied transaction's own `write_set` into one
`Vec<Write>` in block order, and commits it via `StateCommitter::commit`
— the first real caller connecting `apply_block`'s output to an actual
backend. `apply_block` itself is unchanged and still usable standalone
(against a plain `&impl StateReader` with no commit step at all, for a
caller that only wants to simulate/inspect a block without persisting
it — mempool validation is exactly this shape, once it exists).

## Explicitly Not Resolved

**Full ADR-0019 "Atomic State Commit" scope.** Block header, block
body, receipts, events, and consensus metadata are not part of this
commit — none of them are concrete stored objects anywhere in this
codebase yet. Whoever builds those will need to fold them into the same
atomic unit `StateCommitter::commit` now provides for write-sets, likely
by widening `commit`'s own input shape — a real future compatibility
question this ADR does not attempt to pre-answer.

**`compute_state_root`/`state_root` persistence.** `StateCommitter`
commits canonical *values*; it does not compute or store a new state
root, still gated on `compute_state_root` needing the complete current
leaf set (ADR-0007), which requires enumerating everything just
committed plus everything already there — the same "this crate cannot
enumerate storage" structural limit ADR-0032 already named and worked
around for chamber weights. `hn-storage`'s two backends now hold
correct per-key values after a commit; nothing yet re-derives or
verifies the tree root over them.

**Rollback/recovery/pruning/snapshot integration** (ADR-0019's own
"Rollback And Recovery"/"Pruning"/"Snapshot Integration" normative
rules) remain entirely unaddressed — this ADR closes exactly "one
write-set commits atomically," nothing about crash recovery across
commits, retention policy, or snapshot coordination.

## Rejected Options

### A Default `StateWriter::commit_batch` Method Instead Of A New Trait

Rejected: a default method (looping `set`) would compile for every
existing implementor without forcing any of them to reconsider
atomicity — exactly the trap that produces a silently non-atomic
"atomic" commit. ADR-0019's own boundary diagram already names
`StateCommitter` as a distinct interface from `StateWriter`; defining
it as its own trait honors that existing taxonomy instead of collapsing
two named concepts together.

### Widening `commit` To Accept Block Header/Receipts/Events Now

Rejected: none of those have a concrete Rust type or storage design in
this codebase yet — inventing placeholder parameters for them now would
either be dead weight (unused fields) or force premature decisions
about their own storage shape, the same "no implicit/placeholder values
ahead of a real decision" discipline this project applies to economic
parameters, applied here to structural ones instead.

### Making `apply_block` Itself Take A `StateCommitter` And Commit Internally

Rejected: `apply_block` is also useful for pure simulation/inspection
(no persistence at all) — a future mempool or dry-run validator would
call it without ever wanting a commit. Keeping `apply_and_commit_block`
as a separate, additive composing function preserves that usage rather
than forcing every caller to supply a committer it may not want.

## Security Considerations

Partial write-set application on backend failure:

- Risk: without `StateCommitter`, a mid-block I/O failure durably
  persists some but not all of a block's writes, corrupting the
  invariant that state reflects only fully-applied blocks.
- Mitigation: `RedbStateStore::commit`'s single-transaction shape means
  a failure anywhere in the batch leaves the database exactly as it was
  before the call, resting on `redb`'s own transactional guarantee that
  an uncommitted write transaction has no effect — verified directly by
  a test exercising that exact mechanism
  (`an_uncommitted_write_transaction_has_no_effect`), rather than by
  fabricating an artificial mid-batch I/O failure, which `redb`'s own
  API surface gives no reliable, non-fragile way to induce.

Still no atomicity across commits (crash between two `apply_and_commit_block`
calls):

- Risk: this ADR says nothing about recovering "which block was the
  last one fully committed" after an unclean process exit.
- Mitigation: none — ADR-0019's own "Rollback And Recovery" rule stays
  fully open (see "Explicitly Not Resolved"), not silently assumed
  solved by this ADR's narrower guarantee.

## Compatibility

Purely additive: one new trait, two new trait implementations, one new
composing function. No changes to any existing type's wire encoding,
public signature, or behavior — `StateWriter::set`/`StateReader::get`
are untouched.

## Open Decisions

- widening `StateCommitter::commit`'s input to cover block
  header/body/receipts/events/consensus metadata, once those exist as
  concrete stored objects (see "Explicitly Not Resolved")
- `state_root` computation/persistence after a commit (needs a
  storage-enumeration capability this crate still does not have)
- rollback/recovery, pruning, and snapshot integration (ADR-0019's own
  still-fully-open normative rules)

## Related Specifications

- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0019-storage-state-interfaces.md`
- `docs/adr/ADR-0030-transaction-and-block-application.md`
- `docs/adr/ADR-0031-write-set-values-and-overlay-state-reader.md`
