# ADR-0048: Byzantine-Safe Consensus — Lock/Valid State, Vote Journal, Equivocation Evidence, Round Skipping

Status: Accepted

Date: 2026-10-16

Version: 0.2.0

Depends On:

- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0013: Finality Rules
- ADR-0034: Consensus State Machine Skeleton
- ADR-0037: Multi-Node Consensus Wiring
- ADR-0039: Devnet Restart Recovery
- ADR-0042: BlockEnvelope And Receiver-Side Header Verification
- ADR-0045: Transactions Through Consensus

Supersedes: ADR-0034's "Locking rule" and its "Explicitly Not Resolved:
Not byzantine-safe as specified" (the rest of ADR-0034 stays in force
unless this ADR says otherwise)

Referenced By: None yet

## Revision Notes (0.2.0)

Corrections after a review checked 0.1.0 against the code:

1. **Finding 3.4 ("fixed timeouts") was wrong and is removed.**
   `stage_timeout(base_ms, round) = base_ms * (round + 1)` already grows
   the timeout with the round for all three steps. The "Timeouts"
   section now describes the existing mechanism and makes no change
   to it.
2. **The node-side certificate filter was missing from 0.1.0.**
   `expects_qc`/`is_current` in `hn-node` drops any certificate whose
   (height, round) is not current *and* any whose type does not match
   the machine's step (prevote QC only in `Prevote`, precommit QC only
   in `Precommit`). That filter would silently keep R8 and R10 from
   ever running. A new "Decided: Certificate Routing" section covers
   it.
3. **ADR-0009 does not mandate formal modeling.** 0.1.0 (and ADR-0034
   before it) quoted a "Test And Verification Requirements" rule that
   does not exist in ADR-0009; it mentions formal modeling once as one
   mitigation among several and lists the modeling framework as an
   open decision. The verification section no longer leans on a
   mandate and is reordered on technical grounds (exhaustive bounded
   exploration in Rust in CI first, second independent implementation
   as the independent check, TLA+/Quint only as an escalation).
4. **Journal gaps closed:** commit/journal ordering and its crash
   points, a fatal exit for an inconsistent journal, an explicit n = 4
   operational statement, and a hole in the "fresh start" rule.
5. Context now lists the full unlock conjunction.

## Context

ADR-0034 shipped a Tendermint-shaped state machine and named one
simplification: `lockedRound`/`validRound` merged into `highest_qc`.
That ADR labels the skeleton "not byzantine-safe". A line-by-line audit
of `hn-consensus` and `hn-node` (commit `4ea5c4a`) shows the real
picture is different, and worth stating precisely, because it changes
the order of work.

### Finding 1: the merged field is not the safety hole

`highest_qc` is written only together with `locked_block`, only on a
block-targeting prevote quorum seen in step `Prevote`. So
`highest_qc.round` is exactly Tendermint's `lockedRound`. The unlock
test (`qc` is a block-targeting prevote quorum for the proposed block
and `qc.round >= highest_qc.round`) is exactly Tendermint's
`lockedRound <= validRound` test. As far as the safety of the *lock
rule itself* goes, the merge is equivalent. This ADR still separates
the fields, but for **liveness** and clarity, not because the lock rule
was unsafe.

### Finding 2: the real safety holes

1. **No persistence of voting state.** `hn-node` persists committed
   blocks and state (ADR-0039), nothing else. `locked_block`, the round,
   the step, and "what I already signed at (height, round)" live in
   memory. A validator that prevotes/precommits and then restarts can
   sign a *different* vote for the same (height, round, type), or
   forget its lock and prevote a conflicting block. Both break the
   assumption that an honest validator never equivocates and always
   honors its lock. With the f=1 budget of a 4-validator network, one
   such restart is already one "Byzantine" validator that nobody
   intended.
2. **Locking without a validated proposal.** `PrevoteQuorum(block)` in
   step `Prevote` locks and precommits for that block hash even if this
   node never received, or never validated, the proposal behind it
   (`verify_proposal`, ADR-0045). Textbook Tendermint requires holding a
   *valid* proposal for the value before precommitting it.
3. **No equivocation tracking.** `VotePool` keeps one tally per target.
   A validator that signs two different targets at the same
   (height, round, type) is counted in both tallies and nothing records
   the pair. That is safe for quorum arithmetic (quorum intersection
   still holds) but throws away the only evidence that a validator
   misbehaved.
4. **Stale-QC drop.** `node.rs` drops any quorum certificate whose
   (height, round) is not current (`is_current`). Safe, but see
   Finding 3.

### Finding 3: the real liveness holes

1. **The proposer never carries a justification.** Every
   `BlockEnvelope` is built with `justification: None`
   (`node.rs`, `rpc_bridge.rs`). There is no `valid_value`. Scenario:
   round r, f+1 or more honest validators lock on block A (a polka
   formed, but the precommit quorum did not). Round r+1's proposer
   builds a fresh block B. Locked validators prevote nil for B, no
   polka forms, the round advances, and the next proposer again builds
   a fresh block. Nothing ever re-proposes A. Under repeated bad luck
   the height does not finalize. This is the main reason Tendermint
   has `validValue`/`validRound`.
2. **No round skipping.** A validator that is behind in round, because
   it was slow or partitioned, cannot catch up from evidence of
   f+1 validators already in a higher round, and `is_current` drops
   later-round certificates. Together with fixed timeouts this makes
   lagging nodes slow to rejoin and can stall a height.
3. **No precommit-quorum-at-any-round decide.** A precommit quorum for
   a block at an earlier round of the current height is dropped as
   stale, but it is final proof (Tendermint line 49: decide on 2f+1
   precommits for the block at any round, given the proposal). Dropping
   it only delays finality, but it makes late nodes depend on a replay
   through sync.

Note on ADR-0034: its "Explicitly Not Resolved" paragraph cites
ADR-0009 for a formal-modeling requirement that ADR-0009 does not
contain. That sentence is superseded together with the rest of the
paragraph.

The goal of this ADR: specify, precisely enough to implement and test
adversarially, a consensus layer whose **safety** holds with fewer than
one third of voting power Byzantine and arbitrary message delay, and
whose **liveness** holds under partial synchrony with the same bound.

## Decision

The algorithm is Tendermint (Buchman, Kwon, Milosevic, *The latest
gossip on BFT consensus*, 2018). Deviations from the paper are listed
at the end of this section. Nothing in this ADR is a claim of formal
proof; see "Verification Requirements."

### Decided: Adversary And Safety Statement

- `n` validators with voting power `p_i`, `P = Σ p_i`. A quorum is
  `signed_power * 3 > P * 2` (unchanged from ADR-0012).
- Assumption: the Byzantine validators together hold **less than
  `P / 3`**. Anything above that is outside the guarantee.
- **Safety:** no two different blocks are ever finalized at the same
  height. Holds under arbitrary message delay, reordering, duplication
  and loss.
- **Liveness:** after the network becomes synchronous (GST), every
  height eventually finalizes, provided an honest proposer's round
  completes within the round's timeouts. The timeout growth rule below
  makes this true.
- A validator that violates the honest rules below is Byzantine by
  definition. A restart that loses voting state (Finding 2.1) is
  Byzantine behavior from the network's point of view, so durability of
  voting state is part of safety, not an optimization.

### Decided: ConsensusState v2

Replaces the `locked_block` / `highest_qc` pair:

```rust
pub struct ConsensusState {
    pub height: BlockHeight,
    pub round: Round,
    pub step: ConsensusStep,

    // What this validator is bound to. Set only by locking.
    pub locked_value: Option<Digest>,
    pub locked_round: Option<Round>,        // Some iff locked_value is Some

    // Latest value this validator has seen a polka for in this height,
    // locked or not. Set by any valid polka seen in step >= Prevote.
    pub valid_value: Option<Digest>,
    pub valid_round: Option<Round>,         // Some iff valid_value is Some
    pub valid_qc: Option<QuorumCertificate> // the polka itself, for re-proposal
}
```

Invariants (checked by `debug_assert` and by property tests):

- `locked_value.is_some() == locked_round.is_some()`, likewise for valid.
- `locked_round <= valid_round` whenever both are set, and when
  `locked_value` is set, `valid_round >= locked_round`.
- If `valid_value` is set, `valid_qc` is a verified prevote quorum for
  `valid_value` with `valid_qc.round == valid_round`.
- All three reset only on entering a new height. They survive round
  changes. They never reset within a height.

`Option<Round>` is deliberate: Tendermint's `-1` sentinel is exactly
the kind of value that gets compared wrongly. `None` compares as "less
than any round" through explicit helper functions, not through `Ord` on
`Option`.

### Decided: Events Added Or Changed

`ConsensusEvent` gains the following. Every event carries the (height,
round) it refers to, so the machine can do the scoping check itself
(ADR-0034 left this to the caller; this ADR moves it in, see
"Boundary").

| Event | Meaning |
|---|---|
| `Proposal { round, block_hash, valid_round: Option<Round>, justification: Option<QuorumCertificate>, validated: bool }` | A proposal for `round`. `validated` is true only if the caller ran `verify_proposal` (including ADR-0045 execution) successfully. `justification` is the polka for `block_hash` at `valid_round`. |
| `PrevoteQuorum(qc)` | 2f+1 prevotes, any round of the current height. Keeps the existing name; scoping now done inside. |
| `PrecommitQuorum(qc)` | 2f+1 precommits, any round of the current height (see "decide at any round"). |
| `RoundSkip { round }` | Evidence that more than 1/3 of voting power is at `round > self.round` (see "Round skipping"). |
| `*Timeout { round }` | Timeouts now carry their round. A timeout whose round is not the current round is ignored by the machine (returns `Ok(None action)`), replacing the node's `is_current` timer filter. |

### Decided: Transition Rules

Written as the rules the implementation must realize, in the same
notation as the paper. `r` = current round, `lv/lr` = locked value/round,
`vv/vr` = valid value/round. "Cast X" means: emit the action, after the
vote journal write in "Vote journal" has completed.

**R1. Enter propose, `step = Propose`.** If this validator is the
proposer for `(h, r)`:

- if `vv` is set, propose `vv` with `valid_round = vr` and
  `justification = valid_qc`;
- else build a new block from the mempool (ADR-0045), no justification.

Otherwise schedule `ProposeTimeout(r)`.

**R2. Proposal without a polka (`valid_round = None`) in step Propose.**
Let `v` be the proposed value.

- If `validated` and (`lr` is `None` or `lv == v`): cast prevote `v`.
- Otherwise cast prevote nil.

Step becomes `Prevote`.

**R3. Proposal with a polka (`valid_round = Some(vr_p)`) in step
Propose.** Preconditions checked by the machine: `justification` is
present, targets `v`, `justification.round == vr_p`, and
`vr_p < r`. If any fails, the proposal is treated as invalid (prevote
nil). The caller has already verified the QC signatures
(`handle_quorum_certificate`).

- If `validated` and (`lr` is `None` or `lr <= vr_p` or `lv == v`):
  cast prevote `v`.
- Otherwise cast prevote nil.

Step becomes `Prevote`. The condition `lr <= vr_p` is the unlock rule:
a newer-or-equal polka for a different value releases the lock for this
vote only. The lock itself is replaced only by R6.

**R4. Propose timeout** while in step Propose, round `r`: cast prevote
nil. Step becomes `Prevote`.

**R5. Any prevote quorum for nil at round `r`** while in step Prevote:
cast precommit nil. Step becomes `Precommit`.

**R6. Polka for a block `v` at the current round `r`, while step >=
Prevote**, and this validator holds a **validated** proposal for `v`
for round `r`:

- Always: set `vv = v`, `vr = r`, `valid_qc = qc`.
- If step is `Prevote` exactly: also set `lv = v`, `lr = r`, cast
  precommit `v`, step becomes `Precommit`.
- If step is already `Precommit` or later: only the valid-update above;
  no lock, no vote.

If this validator does not hold a validated proposal for `v`, it does
not lock and does not precommit `v`. It records the polka (so a late
proposal can still trigger R6), and continues to wait for the prevote
timeout, which casts precommit nil. This closes Finding 2.2.

**R7. Prevote timeout** while in step Prevote (after any 2f+1 prevotes,
of any targets, were seen and `PrevoteTimeout(r)` fires): cast precommit
nil. Step becomes `Precommit`.

**R8. Decide at any round.** A precommit quorum for a block `v` at any
round `r'` of the current height, plus a **validated** proposal for `v`
(any round): finalize `v` with that certificate. This replaces the
current "only at the current round" rule. If the proposal is missing,
the node records the certificate and fetches the block through ADR-0044
sync (the existing `SKIPPED finalizing ... (no cached proposal)` path
becomes an explicit "need block" state, not a dropped event).

**R9. Precommit quorum for nil, or precommit timeout** at round `r`:
`round = r + 1`, step `Timeout`, then R1 again. (`lv/lr/vv/vr/valid_qc`
are not touched.)

**R10. Round skip.** If the node has seen messages (any proposal, prevote
or precommit, from distinct validators) for a round `r' > r` of the
current height whose senders hold **more than P/3** voting power
together, set `round = r'` and start R1 for `r'`. Only signature-valid
messages from validators in the active set count. A single validator is
counted once per round regardless of how many messages it sent.

R10 is the liveness mechanism for a lagging node. It is safe because
changing round never releases a lock (R9 note).

**R11. New height.** On finalize and commit, all of `lv/lr/vv/vr/
valid_qc` reset, round resets to the first round, step `NewHeight`.
(Unchanged from ADR-0034.)

### Decided: Certificate Routing In `hn-node`

The current `expects_qc` is a caller-side filter with two conditions
that conflict with this ADR: current (height, round) only, and step
must match the certificate type. It was written because ADR-0034's
machine rejects unexpected events with `UnexpectedEvent`. This ADR
replaces it:

1. `hn-node` verifies the certificate (signatures, epoch, set
   commitment, eligibility) and drops it only if its height differs
   from the current height, or it is malformed.
2. Every other certificate is passed to the machine, which owns all
   round and step scoping (R5, R6, R8, R9, R10).
3. The machine distinguishes three outcomes: an action; `Ignored`
   (a new `ConsensusAction::Ignored { reason }`: well-formed but
   redundant, e.g. a prevote polka after the node already moved to
   precommit with the valid state already recorded); and a typed error
   (`WrongHeight`, malformed justification). `Ignored` is distinct from
   `None` so tests can assert it. `UnexpectedEvent` stays for events
   that are impossible in the current step, such as `BeginRound` in
   `Precommit`.
4. The timer filter (`is_current` on timeouts) moves into the machine
   because timeouts now carry their round.
5. A `hn-node` test must prove the filter is gone: a late node
   receives a precommit certificate for an *older round* of the
   current height, plus the block, and finalizes. Without this test an
   ADR-0048 implementation can look complete while R8 and R10 never
   run.

### Decided: Boundary

ADR-0034's boundary ("apply trusts its event completely") is narrowed:

- The machine now **checks** that each event's height equals
  `self.height` and rejects anything else with a typed error
  (`WrongHeight`). It checks round scoping per rule above (R6 current
  round only; R8, R10 any round of the current height).
- The caller still verifies signatures, eligibility, `verify_proposal`
  and quorum arithmetic. The machine takes the caller's word on
  `validated`, so the caller must set it only after a successful
  `verify_proposal`. A test in `hn-node` pins that every proposal path
  sets it from the real verification result.
- The machine never signs. Signing happens in `hn-node` after the vote
  journal write.

### Decided: Vote Journal (Write-Ahead Safety State)

New persistent record, stored in the node's data directory, separate
from the block/state store:

```text
VoteJournal {
    version: u8,
    chain_id, network_id, validator_id,
    height, round, step,
    locked_value, locked_round,
    valid_value, valid_round, valid_qc,
    last_prevote:   Option<(height, round, target)>,
    last_precommit: Option<(height, round, target)>,
}
```

Rules:

1. **Write before send.** Before any prevote or precommit leaves the
   process, the journal is updated with that vote and the new
   lock/valid state, written atomically (temp file, fsync, rename,
   fsync directory), then the vote is signed and sent. A crash between
   write and send loses a vote, which is harmless. A crash that lost
   the write after a send is impossible by construction.
2. **No conflicting second vote.** For a given (height, round, vote
   type), if the journal holds a vote, the node may re-send exactly that
   vote (same target) and never sign a different one. This also
   protects against a bug that tries to sign twice.
3. **Restore on start.** If the journal exists and its height equals
   `latest_finalized + 1`, restore the state machine from it
   (including lock, valid value, round, step). If its height is lower
   (at most `latest_finalized`), it is stale (see "Commit ordering")
   and discarded. If it is higher than `latest_finalized + 1`, the
   block store has lost data. That is **fatal**: the process logs the
   journal height and the store height and exits with a dedicated
   non-zero exit code (the value is fixed in implementation and
   documented). It does not continue as a non-voting observer: running
   in any role on a store that is known to be inconsistent risks
   further corruption and hides the problem from the operator.
4. **Missing or corrupt journal with a non-empty block store.** The node
   cannot know what it signed. It **abstains from voting** through
   height `latest_finalized + 1` and resumes at `latest_finalized + 2`
   (it still follows consensus, applies finalized blocks, and may
   propose only from the resumed height). This costs liveness if more
   than one third of power loses journals at once, which is an operator
   error budget, documented, not silently risked.
5. **Genesis start** (empty store, empty journal) votes normally only
   if it is a genesis bootstrap: every peer the node handshakes with
   reports finalized height 0. A node with an empty store that sees
   any peer above height 0 is a validator that lost its data or a new
   one: it syncs first (ADR-0044, observe only), then abstains through
   the height after the one it synced to, exactly as in rule 4. A
   Byzantine peer that lies about its height to push a rejoining node
   into voting is a residual risk, see Open Decisions.
6. The journal stores no secret key. Key storage remains the separate
   keystore ADR.

**Commit ordering.** The journal and the block store are two separate
files, so no atomicity between them is assumed. The ordering rule is
instead:

- The journal for height `h + 1` is created only **after** block `h` is
  committed. A vote at height `h + 1` can therefore never exist while
  the store is still at `h - 1`.
- Crash after the store committed `h` but before the journal moved on:
  the journal says `h`, `latest_finalized + 1` is `h + 1`, rule 3
  discards it. Safe, because the node had not voted at `h + 1` (it
  cannot, per the previous bullet).
- Crash while committing `h` (store transaction not durable): the store
  is at `h - 1`, the journal at `h` is valid and is restored, the node
  re-enters height `h` with its lock intact and finalizes `h` again from
  the certificate or sync.
- The store transaction is atomic by ADR-0033 (single redb
  transaction), so "half-committed" is not a state.

Devnet's "kill a node and restart" tests (`restart_recovery*`) must
keep passing, and new tests kill a node *between* voting and
finalization (see Verification).

### Decided: Equivocation Evidence

`VotePool` changes from per-target tallies to per-validator first-seen
records:

- For each (height, round, vote type, validator) store the first
  verified vote seen. A later verified vote from the same validator at
  the same key with a **different** target is **not counted** and is
  recorded as `EquivocationEvidence { vote_a, vote_b }`. A repeat with
  the same target is idempotent.
- Evidence is kept in memory and logged (`EQUIVOCATION validator=...
  height=... round=... type=...`) and exposed on RPC `GET /v1/evidence`
  only if a later RPC ADR adds it. This ADR does not add the endpoint.
- **No slashing, no inclusion of evidence in blocks.** Consequences
  (stake loss, jailing, who may submit evidence) are economic and
  governance decisions and fall under the Economic Decision Constraint.
  Recording evidence now is free and keeps the option open.
- Counting only the first vote is safe: a Byzantine validator gains
  nothing in the tally, and honest validators never equivocate.

### Decided: Proposer Duties

- The proposer re-proposes `valid_value` with its polka when it has one
  (R1). This is the fix for Finding 3.1.
- `round_proposer` stays the placeholder `(height + round) % len`
  (ADR-0011 is still open). The safety argument above does not depend
  on how proposers are chosen. Liveness needs only that every
  validator is eventually the proposer for some round and that a
  Byzantine proposer is followed by a different one.
- A proposer may not send two different proposals for the same
  (height, round). A receiver that sees two signed proposals from the
  legitimate proposer for the same (height, round) records them as
  `EquivocationEvidence` (type proposal) and prevotes only for the
  first one it validated.

### Decided: Timeouts (Existing Mechanism Kept)

`stage_timeout(base_ms, round) = base_ms * (round + 1)` already grows
every step's timeout with the round, which is what partial synchrony
needs. This ADR changes nothing here except that timeout events now
carry their round (see events). Whether to split the single base into
per-step bases, or move to additive growth, is a tuning question for a
measured testnet, not a gap. Values remain local node configuration,
not protocol parameters.

### Decided: Gossip Requirements Safety Depends On

Tendermint liveness assumes eventual delivery of messages from honest
validators. The current network layer gossips once per vote with no
retry. For this ADR to hold in the wild:

- A validator must re-broadcast its latest prevote/precommit for the
  current (height, round) on a timer while it has not advanced, and
  answer a peer that reports being on a lower round with the data to
  catch up (the round's proposal and the polka or precommit
  certificates it holds).
- This is a **liveness** requirement (Finding 3), not a safety one, and
  its concrete wire format belongs to a P2P ADR. This ADR records the
  dependency so it is not forgotten: until it exists, a single dropped
  vote message can stall a round until timeouts, and devnet already
  shows this class (SKIPPED finalizing).

### Deviations From The Paper

1. Voting power weights instead of one-validator-one-vote. Thresholds are
   `> 2/3` and `> 1/3` of `P`.
2. Proposals carry a `BlockEnvelope` with a mandatory `verify_proposal`
   including execution (ADR-0045), so "valid(v)" in the paper is the
   result of that function. The machine receives it as `validated`.
3. Decide-at-any-round needs the block, which may arrive via ADR-0044
   sync rather than gossip.
4. Round skip uses messages of any type, as in the paper's `f+1` rule;
   no separate timeout certificate (ADR-0009).
5. The vote journal is an implementation requirement added by this ADR;
   the paper assumes processes do not forget (crash-recovery model).
6. Evidence is recorded but not acted on.

### Operational Statement For n = 4

With four equal validators, `f = 1` and a quorum is exactly 3 of 4: no
spare. Consequences, stated so nobody finds them in production:

- One validator that lost its journal (rule 4) abstains for one
  height. During that height the other three must all be up, in the
  same round, with no message loss. The fault budget of the whole
  network is spent on that single node for one height. A second
  unrelated problem (a slow node, a dropped vote) stalls the height
  until timeouts or a later round succeed.
- This is acceptable for devnet and must not be copied to a mainnet
  profile. Mainnet validator-set size and the operator rule "never
  wipe a journal on a running network" belong to the launch plan.
- It is still better than the alternative. Voting without knowing
  what was signed risks an actual double-sign and breaks safety, not
  just liveness.

## Rejected Options

### Keep `highest_qc` And Only Add A Journal

Rejected. It would fix the restart hole but leave Finding 3.1: after a
partial lock no one re-proposes the locked value, so liveness depends
on luck. Separate valid state is the cheap fix with the biggest effect.

### Track Only Locks, Skip The Journal Because Devnet Is Trusted

Rejected. The journal costs a few dozen lines and one fsync per vote.
Retrofitting it after testnet means every validator operator has
already learned to "just restart the node". Also, a restart losing
state is the most likely real-world way to produce a double-sign.

### Slash Or Jail On Evidence Now

Rejected for this ADR. Needs penalty size, who submits evidence,
evidence age limits, and interaction with unbonding. All economic
decisions. Evidence recording stays so these can be added later.

### Formal Proof Inside This ADR

Rejected. This is textbook Tendermint plus the listed deviations, so
the work is adversarial testing and checking the deviations, not a
new proof. ADR-0009 sets no formal-modeling mandate (it lists the
modeling framework as an open decision), so tooling is chosen on
technical merit; see Verification Requirements.

### TLA+/Quint As The Primary Check

Rejected as the baseline. At n = 4, f = 1 it checks the same finite
case an exhaustive Rust exploration checks, it adds a toolchain nobody
on the project uses yet, and by its own nature it would be run by hand
and drift from the code. It stays available as an escalation.

### Time-Based (Wall-Clock) Round Synchronization

Rejected. Round skipping from f+1 evidence is simpler, needs no clock
assumptions, and is what the paper uses.

## Security Considerations

- Journal tampering: an attacker with write access to the data
  directory can rewind a journal and induce double signing. This is the
  same threat class as stealing the signing key and is out of scope
  here. File permissions follow the keyfile rules (owner-only).
- Journal as a liveness risk: missing journal means abstaining through
  one height (rule 4). Operator tooling must make "restore from a
  backup journal" impossible to do wrongly. A restored *old* journal
  is more dangerous than a missing one: rule 3's height check discards
  stale ones, but an old journal for the same height with fewer votes
  is indistinguishable from the real one. Operators must never copy
  journals between machines or restore them from backups. This warning
  goes into the operator documentation when it exists.
- Round skip amplification: counting only signature-valid messages from
  active validators, once per validator per round, bounds what an
  outside attacker can force.
- Memory: the vote pool and evidence store grow without bound today
  (ADR-0037, stale entries not pruned). Prune by height after finality
  as part of implementation; evidence retention length is a separate
  decision.
- Proposal execution cost is bounded as in ADR-0045; with round-growing
  timeouts a malicious proposer cannot force a round shorter than
  execution time because rounds only get longer.
- Sync interplay: R8 lets a node finalize from a precommit certificate
  it received at an older round. It still needs the block and the
  ADR-0044 five-step verification; the certificate alone never commits
  state.

## Compatibility

Pre-mainnet, devnet only. The vote and certificate wire formats do not
change. `ConsensusState`, `ConsensusEvent` and the engine API change
(Rust API, not wire). A new on-disk file (the journal) appears next to
the data directory. Existing devnet data directories have no journal
and fall under rule 4 (abstain through one height) on first start; the
test harness creates fresh directories, so no test is affected. The
`ConsensusState` public fields change, so `hn-node`'s uses of
`locked_block`/`highest_qc` (tests only today) are updated.

## Verification Requirements

Safety claims must be demonstrated, not argued.

1. **Unit tests in `hn-consensus`**, one per rule R1–R11, including the
   negative cases: lock protects against a different value, polka at a
   newer round unlocks the vote but not the lock, polka without a
   validated proposal does not lock, valid update after step moved on,
   round skip needs more than one third, nothing resets within a
   height.
2. **Deterministic adversarial simulator** (new test support, in-process,
   no sockets): n = 4 (also 7), f = 1 (also 2), the harness controls
   message delivery order, delay, duplication and drop, and scripts
   Byzantine validators (equivocate, stay silent, propose conflicting
   blocks to different peers, vote for a block never proposed).
   Required scenarios:
   - equivocating proposer cannot cause two finalizations;
   - the partial-lock scenario of Finding 3.1 finalizes, thanks to
     re-proposal of the valid value;
   - lagging validator catches up through round skip;
   - precommit quorum from an older round finalizes a late validator;
   - a Byzantine validator that double-votes yields recorded evidence
     and does not change any tally.
3. **Exhaustive bounded exploration, in CI.** The same simulator, driven
   by an explorer that enumerates delivery schedules instead of
   sampling them: n = 4, f = 1, at most 2 heights worth of rounds
   (bound: 3 rounds, chosen so a CI run stays under a few minutes; a
   larger bound runs nightly or on demand), Byzantine validator
   behaviors drawn from a fixed menu (silent, equivocate, vote for
   anything). After every step assert "no two different blocks
   finalized at the same height" and the state invariants. A violation
   prints the schedule and is saved as a named regression test. On top
   of that, a randomized run (fixed seeds recorded, thousands of runs)
   covers n = 7 and longer horizons where full enumeration is too big.
   The bounds are part of the test's documentation: the result is
   "safe within these bounds", never "safe".
4. **Crash tests for the journal.** Drop all in-memory state of a
   validator at every point between "journal written", "vote sent" and
   "next event", **and** at the commit boundary: before the block store
   commits `h`, after it commits but before the journal advances,
   after the journal advances. After restart it must not sign a
   different vote for the same (height, round, type), must keep its
   lock, and must never vote at a height above `latest_finalized + 1`.
   One real-process test: 4 processes, kill one right after it
   prevoted, restart it, assert the network still finalizes and the
   restarted node emits no conflicting vote (the node logs every signed
   vote; the test compares them). One more: truncate the block store
   under a valid journal and assert the fatal exit of rule 3.
5. **Independent second implementation.** A second, deliberately small
   (about 150 to 250 lines) Rust implementation of R1 to R11, written
   from this text by someone who has not read the first (or by a
   separate agent session with only the ADR), no network, crypto or
   execution. The explorer from item 3 runs both implementations on the
   same schedules and requires identical outputs (actions and state)
   at every step. This catches the specific risk that the code and
   spec drifted or share a blind spot, and it runs in CI like any
   other test. It is the independence a separate modeling tool would
   have given, without a new toolchain.
6. **Escalation, not baseline.** If items 3 and 5 leave a deviation
   that cannot be reasoned about by hand (round skip interacting with a
   lock, journal restore interacting with round skip), model that part
   in TLA+ or Quint. Record the reason, the model and the command to
   reproduce in `docs/`.
7. **CI**: items 1 to 5 run in CI. A larger-bound exploration runs
   nightly or manually and its bounds and result are logged in `docs/`.

## Open Decisions

- Real leader election and proposer rotation (ADR-0011). Independent of
  this ADR's safety argument.
- Gossip retry / catch-up wire messages (P2P ADR).
- What to do with equivocation evidence: inclusion in blocks, slashing,
  jailing, evidence age (economic / governance).
- Operator tooling around the journal (backup warning, a sanctioned way
  to migrate a validator).
- Timeout base values for testnet and mainnet (the growth rule exists).
- Rejoining validator that lost both store and journal on a running
  network, when peers lie about their height: needs checkpoints or
  weak-subjectivity rules (separate ADR with light-client / checkpoint
  work).
- Mainnet validator-set size and the operator procedure around
  journals (see the n = 4 statement).
- Validator-set changes at epoch boundaries interacting with a polka
  formed under the old set (quorum certificates already carry
  `validator_set_commitment`; confirm the machine rejects a
  certificate from a different epoch's set).
- `FinalityProof` concrete type (carried over from ADR-0034).

## Related Specifications

- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0034-consensus-state-machine-skeleton.md`
- `docs/adr/ADR-0039-devnet-restart-recovery.md`
- `docs/adr/ADR-0045-transactions-through-consensus.md`
- Buchman, Kwon, Milosevic, *The latest gossip on BFT consensus*
  (arXiv:1807.04938)