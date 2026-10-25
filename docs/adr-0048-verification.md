# ADR-0048 verification record

What ADR-0048's own "Verification Requirements" section asked for,
what actually exists against each item, and where. This file is the
project-level record, the same role `docs/fuzzing.md` plays for the
fuzz target — the ADR itself stays the design decision, this stays the
evidence.

## Status by item

1. **Unit tests in `hn-consensus`, one per rule R1-R11, including the
   named negative cases.** Done. `hn-consensus/src/state.rs`'s own
   `#[cfg(test)] mod tests` (commits `c09be08`, `1dd2f80`).

2. **Deterministic adversarial simulator, the five required
   scenarios.** Done. `hn-consensus/tests/support/mod.rs` (the
   harness) and `hn-consensus/tests/byzantine_simulator.rs` (the
   scenarios: equivocating proposer, the partial-lock re-proposal
   case, lagging-validator round skip, precommit-from-an-earlier-round,
   double-vote evidence) — commits `1b637ea`, `27a5dfb`.

3. **Exhaustive bounded exploration in CI, plus a randomized run for
   `n = 7`.** Done. `hn-consensus/tests/exhaustive_exploration.rs`
   (n = 4, f = 1, 3-round bound, 69,632 schedules, ~0.2s release) and
   `hn-consensus/tests/property_safety.rs` (2,000 seeds each for n = 4
   and n = 7) — commits `4d0cd0e`, `a7bb1fa`. Both run as part of the
   ordinary `cargo test --workspace` CI step; no separate gate. A
   larger-bound nightly/manual run (item 7 below) has not been set up
   yet — open, not forgotten.

4. **Crash tests for the journal**, both named cases:
   - "Drop all in-memory state... at every point between 'journal
     written', 'vote sent' and 'next event', and at the commit
     boundary." The commit-boundary scenarios are covered as named
     unit tests against the pure decision table
     (`hn-consensus/src/journal.rs`'s own `commit_ordering_*` tests,
     commit `f9fd419`) and against the real file I/O
     (`hn-consensus/tests/journal_durability.rs`, commit `63109b8`).
   - "One real-process test: 4 processes, kill one right after it
     prevoted, restart it, assert the network still finalizes and the
     restarted node emits no conflicting vote."
     `hn-node/tests/restart_after_prevote_no_conflicting_vote.rs`
     (commit `276bf00`). Writing it surfaced a real crash (restoring
     mid-round unconditionally called `begin_round()`, which has no
     defined transition except from `NewHeight`/`Timeout`) — fixed in
     the same commit (`resume_or_begin_round`), which also made rule
     2's "the node may re-send exactly that vote" a real resend on
     resume, not only a defensive check.
   - "One more: truncate the block store under a valid journal and
     assert the fatal exit of rule 3."
     `hn-node/tests/journal_store_mismatch_is_fatal.rs` (commit
     `276bf00`).

5. **Independent second implementation, diff-tested against the
   real one.** Done. An agent session was given only ADR-0048's own
   transition-rule text and bare primitive type shapes, explicitly
   forbidden from reading any existing `hn-consensus` source, and
   produced a ~560-line second implementation
   (`hn-consensus/tests/independent_impl_diff.rs`), diff-tested
   against the real `ConsensusState` on 20,000 random schedules —
   commit `a63321f`. Found three real bugs in the production
   implementation (a step-mismatched `Proposal`/`*Timeout` hitting a
   hard error instead of `Ignored`; a round-keyed proposal cache that
   forgot an early-arriving validated proposal; a stale
   `pending_finalization` left over after finalizing a different
   certificate), all fixed in the same commit, plus two corrections to
   the independent copy itself (documented in its own module doc).

6. **Escalation to TLA+/Quint, only if items 3 and 5 leave a deviation
   that cannot be reasoned about by hand.** Not triggered. Every
   deviation items 3 and 5 actually surfaced (the four bugs above, and
   the `resume_or_begin_round` crash item 4 surfaced) was understood,
   fixed, and re-verified by the same bounded exploration / diff test
   / real-process test that found it — none of them needed a separate
   formal model to reason about. The two round-skip/lock interactions
   the ADR itself names as candidates (round skip interacting with a
   lock; journal restore interacting with round skip) remain
   candidates for exactly this treatment once R10 (round skip from
   gossiped evidence, see "Open scope" below) is actually wired —
   revisit this item then, not before.

7. **CI**: items 1-5 run in CI as part of the ordinary `cargo test
   --workspace --all-targets --all-features` step — nothing here needs
   a dedicated workflow job, unlike the fuzz target. The larger-bound
   nightly/manual exploration run this item also calls for is still
   open (see item 3).

## Open scope

Two pieces of ADR-0048's own design were deliberately not wired into
`hn-node` in this pass — not bugs, missing wire-protocol work the user
chose to scope separately:

- **R1** (the proposer re-proposes `valid_value` with its own polka as
  `justification`). This codebase's `block_hash` is the hash of the
  *entire* `BlockHeader`, including `round` — re-proposing the same
  value at a later round with the same `block_hash` (required for R3's
  own `justification.target_hash == block_hash` check to ever pass)
  means resending the original envelope byte-for-byte, with its
  original round still inside the header, while the message itself is
  understood as "this round's proposal." Today's `ConsensusProposal`
  wire message has no field for that distinction at all — the round it
  is for is read straight off the embedded header's own `round`. Needs
  a real wire-protocol addition, not an internal refactor.
- **R10** (round skip from gossiped evidence: more than one third of
  voting power already at a later round). Nothing in `hn-node` tallies
  a peer's own current round with any weight at all today — this needs
  a new gossip mechanism, not a stub waiting to be filled in.

Both are real, named gaps, not oversights — tracked here so "ADR-0048
done" never silently comes to mean "done except for the two hardest
parts."
