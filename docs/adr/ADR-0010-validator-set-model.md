# ADR-0010: Validator Set Model

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture

Supersedes: None

## Context

The validator set defines which participants may propose, vote on, and finalize
blocks for a consensus period.

HNChain should support broad validator participation, but validator count cannot
be chosen as a slogan. The active set size, voting power model, hardware
requirements, signature aggregation, networking latency, and finality target are
coupled design parameters.

The validator set model must be explicit before leader election, quorum
certificates, slashing, staking economics, and light-client finality proofs can
be finalized.

## Decision

HNChain defines a versioned validator set model with separate concepts for:

- validator account
- candidate validator
- active validator
- inactive validator
- jailed validator
- exited validator
- validator set commitment

Conceptual lifecycle:

```text
Registered
  -> Candidate
  -> Active
  -> Inactive
  -> Jailed
  -> Exited
```

The validator set used for consensus at a height is derived from canonical state
and committed through block consensus metadata.

**Decided: capped stake-weighted voting power.** Asked the user first —
this decision carries the same weight as choosing the consensus family
itself (ADR-0009): it is not forced by anything already decided, and
this ADR's own "Rejected Options" already refuses to accept plain
stake-weighting as a default without centralization analysis.

`voting_power(validator)` is proportional to bonded stake, bounded by a
maximum share of total voting power — not plain stake-weighting (this
ADR's own "One Coin Equals One Vote As Default" rejection: "direct
stake weight without caps or delegation design can increase governance
and consensus concentration risk"), and not equal weight (would need
its own separate Sybil-resistance mechanism to replace what stake
already provides, and makes active-set admission a much more
consequential, politically-sensitive decision than under a
stake-weighted model — "Equal Weight Active Validators," Alternatives
Considered).

**Committee-Based Active Set is ruled out**, not on its own merits but
by an already-made decision: it "can reduce per-block voting overhead"
but needs "committee selection randomness" (Alternatives Considered,
below) to choose the committee — and ADR-0011 ("Decided: deterministic
weighted round-robin") already committed this profile to *no*
randomness source anywhere in consensus. A committee-based voting
power model would silently reopen a question ADR-0011 already closed.

The **exact cap value**, the **precise capping algorithm** (a simple
"stake-weighted, then truncate at a share of the total" pass is not
obviously stable, since capping the largest validators changes the
total the cap itself is computed against — a fixed-point/water-filling
subtlety, not a detail this decision resolves), Sybil-resistance rules,
delegation design, and minimum bond all remain open (Open Decisions,
below) — deciding the *model* does not require resolving its exact
parameters or capping mechanics, the same scoping already used for
`MAX_TRANSACTION_SIZE`-class decisions.

## Normative Rules

### Versioned Validator Records

Every validator record includes a schema version.

Nodes must not infer validator semantics from account type, address prefix,
client software, public key length, or RPC metadata.

### Validator Identity

Validator identity is a protocol identity bound to a validator account.

Required conceptual fields:

```text
ValidatorRecord
  record_version
  validator_id
  consensus_key
  network_key
  status
  bonded_stake
  voting_power
  activation_epoch
  deactivation_epoch
  metadata_hash
```

`account_address` (present in this struct's earliest conceptual sketch)
is dropped — see "Decided: `validator_id` derivation," below:
`validator_id` *is* the controlling account's own address, so a
separate field would duplicate it. `bonded_stake` is added — see
"Decided: `bonded_stake`, distinct from `voting_power`," below.

`consensus_key` signs consensus messages.

`network_key` authenticates peer-to-peer node communication when required.

Operational keys must be rotatable without changing account ownership.

**Decided: `validator_id` derivation.** Asked the user explicitly —
found while trying to write concrete payload shapes for ADR-0006's
`stake`/`unstake`/`validator_update`, which need to name a target
validator and settle who may act on it, neither previously resolved.
`validator_id` is the controlling account's own `address_body` (ADR-0003,
`account` namespace) — `hn_crypto::account_address_body`'s existing
output, no new derivation function. This resolves two things at once:

- **Stability across consensus key rotation** (the reason `validator_id`
  was kept separate from any key-derived address in the first place,
  ADR-0012, "Decided: `validator_id` width, not its exact derivation"):
  an account-derived `validator_id` never depends on `consensus_key` at
  all, unlike `hn_crypto::validator_address_body` (ADR-0003, `validator`
  namespace), which *is* derived from `consensus_key` and rotates with
  it. The two now have clearly distinct, non-overlapping purposes:
  `validator_id` is the stable protocol identity a record lives at;
  `validator_address_body` names a specific key epoch, useful where
  that distinction matters (for example network-layer peer identity)
  but not for locating a validator's own record.
- **Authorization for validator-management transactions.** A
  transaction's `sender` is already `bytes32` — the sender account's
  own `address_body` (ADR-0006, "Chain And Network Binding"). For a
  self-managed validator, `sender == validator_id` directly, with no
  separate ownership field needed anywhere: authorization is "the
  transaction that names this `validator_id` was sent by the account
  that *is* this `validator_id`." Third-party/delegated bonding (an
  account managing a `validator_id` it is not itself) is out of scope
  here — that is ADR-0010's own already-open "delegation support," not
  reopened by this decision.

This also resolves this section's own conceptual `account_address`
field as redundant: once `validator_id` literally *is* the controlling
account's address, a separate stored `account_address` field would
duplicate it — the eighth instance this session of the
`protocol_name`/`checksum_profile`/`hash_profile`/`signing_purpose`/
`quorum_threshold`/`key_role`/`verification_context` redundancy class.
Dropped from the conceptual struct above and from any future concrete
`ValidatorRecordV1` field list.

**Decided: `bonded_stake`, distinct from `voting_power`.** Also found
while writing `stake`/`unstake`: those transactions need a field to
modify directly, and none exists — `ValidatorRecordV1`'s only numeric
field today is `voting_power`, which is not that field. The capping
algorithm's own already-decided design (above, "Decided: capping
algorithm") computes `power_i = min(stake_i, C_r)` — `stake_i` (raw
bonded stake, per validator) and `power_i` (capped voting power, the
algorithm's output) are different values whenever a validator's raw
stake exceeds the current cap, and `C_r` depends on the *entire*
candidate set's total, not any one validator in isolation. This means
`voting_power` cannot be recomputed synchronously as part of a single
validator's own `stake`/`unstake` transaction — only `bonded_stake`
can change at that point; `voting_power` reflects whatever the capping
algorithm's last full-candidate-set run computed (for example at the
next epoch boundary, alongside `ACTIVE_SET` selection — exact timing
not decided here). `ValidatorRecordV1` needs both fields: `bonded_stake`
(raw, `u128`, matching `native_balance`'s own precedent — directly
mutated by `stake`/`unstake`) and `voting_power` (capped, `u128`,
already decided — mutated only by the capping algorithm's own
recomputation, never directly by a transaction).

### Validator Status

Validator status is consensus state.

Initial conceptual statuses:

- `registered`
- `candidate`
- `active`
- `inactive`
- `jailed`
- `exited`

Status transitions must be deterministic and authorized by account permissions
or protocol rules.

**Decided: admission mechanism (`registered → candidate → active`).**
Continues directly from "Active Set Derivation," below, which already
resolves the question the old "validator admission ranking" Open
Decision implied needed answering: once the active set is bounded and
selected every epoch by `voting_power` (top-K, decided below), there is
no separate competitive or ranked admission step left to design —
whether a validator actually *participates* in a given epoch is already
fully handled by that per-epoch selection, not by a one-time admission
gate. "Ranking" as a name for this Open Decision was a leftover from
before that decision existed; what is actually left to define is purely
mechanical.

Grounded in `validator-set.md` §11's already-named conceptual operation
registry (`validator_register`, `validator_bond`, `validator_activate`,
`validator_deactivate`, ADR-0006) rather than invented from nothing:
only `activate`/`deactivate` are named status-transition operations —
there is no `validator_candidate` operation anywhere in that registry.
This asymmetry is itself informative, not an oversight to fix:

- `registered → candidate` is **automatic**, not transaction-triggered:
  a `registered` validator's status is `candidate` whenever its bonded
  stake (`validator_bond`) meets the still-open minimum bond
  requirement — a derived condition read from canonical state, the same
  way `EnvelopeValueV1`'s section state is read rather than separately
  flagged. No operation exists to request it because none is needed.
- `candidate → active` is **explicit**, via `validator_activate`: the
  validator must deliberately opt in, rather than being silently
  drafted into consensus duty the moment a bond threshold is crossed.
  This matters increasingly once ADR-0015 (Slashing And Accountability,
  still fully open) activates real penalties for active-duty failures —
  an operator should choose when their infrastructure is ready to accept
  that exposure, not have it imposed by a balance check alone.
  `validator_activate`'s minimum-bond precondition is checked against
  the same still-open minimum, without this decision fixing its value.
- **Timing reuses "Epoch Boundaries," below, at individual-validator
  granularity rather than introducing a second delay constant.**
  `validator_activate` (and symmetrically `validator_deactivate`) may be
  submitted at any height, but its status effect lands only at the next
  epoch boundary — exactly the same one-epoch lead time already decided
  for validator *set* transitions generally, applied here to one
  validator's own status instead of the aggregate set. This resolves
  "activation delay" and "deactivation delay" (Open Decisions) as a
  *mechanism* — one epoch, no new tunable — without needing their own
  separate constant. **Unbonding period stays genuinely open, not
  resolved by this**: it governs when bonded funds may be *withdrawn*
  after becoming `inactive`/`exited`, a distinct, security-motivated
  window from mere status/consensus-participation timing (matching
  real-world unbonding periods measured in weeks, not one epoch).
- **Jailing is deliberately untouched here** — `active → jailed` is a
  penalty-driven transition, not an admission concern, and belongs to
  ADR-0015 (Slashing And Accountability), the other structural fork the
  user named as a live candidate alongside this one. "Jailing
  conditions" (Open Decisions) stays open.

Not resolved by this decision: the minimum bond's numeric value (a
deferred economic parameter, same group as `MAX_ACTIVE_SET_SIZE` and
`cap_numerator`/`cap_denominator`), the concrete transaction schemas for
`validator_register`/`validator_bond`/`validator_activate` (ADR-0006's
own still-open "final validator operation transaction schemas"), and
whether bonded stake falling below the minimum *after* activation has
any automatic effect on status (a real open question, but a distinct
one from admission — not invented here).

### Active Set Derivation

The active validator set for an epoch or height must be derived deterministically
from canonical state.

Derivation must define:

- eligible statuses
- minimum stake or bond rule, if any
- ranking rule, if active set size is bounded
- tie-breaking rule
- activation delay
- deactivation delay
- maximum active set size, if any
- voting power calculation

**Decided: active set derivation mechanism.** Three of the items above
are already settled by decisions made elsewhere, not independent
choices here:

- **Eligible statuses** — `active` only, by definition: the lifecycle
  name itself is what "currently participates in consensus" means
  (Decision, "Validator Status," above). `registered`/`candidate` have
  not yet been admitted; `inactive`/`jailed`/`exited` have left or been
  suspended. How a validator *reaches* `active` (the bond check at
  `candidate → active`) is decided separately ("Decided: admission
  mechanism," "Validator Status," above) — this decision only says
  which status counts toward the active set, not how a validator earns
  it.
- **Input state / timing** — already fixed by "Epoch Boundaries," above:
  a height's active set is whichever epoch's already-snapshotted set
  that height falls under (`BlockHeader.epoch`, ADR-0008, names which
  one), snapshotted a full epoch ahead of activation. No separate
  timing rule needed here.
- **Deterministic output ordering** — ascending `validator_id`, reused
  from `validators_root`'s and `signer_commitment`'s own already-decided
  order (ADR-0010's "Validator Set Commitment," ADR-0012's "signer
  commitment bit-level encoding") rather than inventing a third
  canonical order for the same conceptual list.

What remained a genuine fork, not derivable from precedent: **is the
active set bounded (top-K by a ranking) or unbounded (every
sufficiently-bonded `active` validator)?** Asked the user explicitly —
this is not mechanically forced by anything already decided, unlike the
three items above. Decided: **bounded, top-K by `voting_power`
descending**, ties broken by ascending `validator_id` (reusing leader
election's own tie-break rule, ADR-0011, rather than inventing a second
one). Directly consistent with two things already on record rather than
picked from nothing: the QC aggregation-scheme decision (ADR-0012)
justified individual-signatures-with-bitmap specifically by
"certificate size is `O(n)` in active set size, acceptable while the
active set stays modest" — a claim that only holds if the set is
actually capped; and this ADR's own "Tendermint-Style BFT" Alternatives
Considered entry (ADR-0009) already names "very large validator sets are
difficult without aggregation or committee mechanisms" as this profile's
known disadvantage, never resolved, just accepted for a modest set.

```text
ACTIVE_SET(epoch) -> Vec<ValidatorRecordV1>
  candidates = { v : v.status == active }
  ranked = candidates sorted by (voting_power desc, validator_id asc)
  selected = ranked.take(MAX_ACTIVE_SET_SIZE)
  ACTIVE_SET = selected, re-ordered ascending by validator_id
```

`MAX_ACTIVE_SET_SIZE` itself (the cap value `K`) is deliberately not
decided here — same scoping this ADR already used for the capping
algorithm versus `cap_numerator`/`cap_denominator`'s value: the
*mechanism* is decided, the *parameter* stays open ("initial active
validator set size policy," Open Decisions). Note the final
re-ordering step: ranking (by `voting_power`) and output order
(by `validator_id`) are two different orderings serving two different
purposes — selecting *who* makes the cut, versus a canonical,
content-independent order for everything downstream (`validators_root`,
`signer_commitment` bit indices) that does not change every time
relative stake shifts.

### Voting Power

Voting power is a consensus value.

Every accepted consensus profile must define:

- voting power source
- integer type and bounds
- zero-power behavior
- rounding behavior
- overflow behavior
- total power calculation
- quorum threshold calculation

Floating-point arithmetic is rejected for voting power.

Voting power source is decided above (Decision: capped stake-weighted).
The quorum threshold calculation itself is already decided independently
of everything below (ADR-0012, "Decided: quorum threshold formula":
`signed * 3 > total * 2`, exact integer arithmetic, applies unchanged
regardless of voting power's width or source).

**Decided: voting power integer type — `u128`.** Verified against the
codebase rather than assumed, per the same discipline used for every
prior decision today: `hn_core`'s protocol *counters*
(`BlockHeight`/`Epoch`/`Round`/`ProtocolEpoch`/`AccountNonce`) are all
`u64` newtypes, but this project's *economic amounts*
(`BalanceValueV1.native_balance`, `AssetValueV1.holdings`'s per-entry
amount) are plain `u128`, with no dedicated wrapper type. `voting_power`
belongs to the second category, not the first — it is capped bonded
stake (Decision, above), and the capping algorithm decided just above
this operates directly on `stake_i` with no rescaling step, so its
natural output unit is bonded stake's own unit. `u64` would have been a
category error, not just an unverified guess: it would silently assume
a normalization step this ADR never decided. `voting_power` stays a
plain `u128` field on `ValidatorRecordV1`/`ValidatorSetCommitmentV1`/
`QuorumCertificate`, matching `native_balance`'s own precedent of no
dedicated newtype for an amount-shaped value.

**Decided: overflow behavior — checked, not wrapping.** `total_voting_power
= sum(power_i)` and the quorum formula's `signed_voting_power * 3` /
`total_voting_power * 2` must use checked arithmetic; overflow is
rejected as invalid, never silently wrapped, matching the
`checked_add`/`checked_sub` pattern `apply_transfer` (ADR-0006) already
establishes for balance arithmetic. In practice this is headroom, not a
live risk: `total_voting_power` is bounded by the sum of bonded stake
across the active set, itself bounded by total token supply, and
`native_balance` was already given `u128` "with headroom" specifically
so this kind of downstream arithmetic would not need to worry about
realistic overflow — checked arithmetic here is a correctness
requirement (ADR-0000, "No Hidden Consensus Dependencies": overflow
behavior must be explicit, not implementation-defined), not evidence
that overflow is expected.

**Decided: rounding behavior — none beyond the capping algorithm's own
`floor`.** No other operation on `voting_power` performs division:
`power_i = min(stake_i, C_r)` and `total_voting_power = sum(power_i)`
are both exact integer operations, and the quorum formula was already
decided to use multiplication specifically to avoid needing rounding at
all (ADR-0012). The capping algorithm's `floor(cap_numerator * total_r /
cap_denominator)` (Decision, above) is the only rounding rule this
profile needs.

Voting power's *bounds* (a maximum value below `u128::MAX`, if any) and
zero-power behavior remain open, below — narrower questions than the
type itself, not blocking `QuorumCertificate` or `ValidatorRecordV1`
from being concretely encodable.

**Decided: capping algorithm (mechanism, not the cap fraction's
value).** Asked the user explicitly — this is the exact fixed-point
subtlety flagged as unresolved above, and a naive implementation
contains a real bug: a single-pass `power_i = min(stake_i,
cap_fraction * total_raw_stake)` does not actually bound any
validator's post-normalization *share*. Example: `cap_fraction = 20%`,
raw stakes `[80, 10, 10]` (`total_raw_stake = 100`) gives
`C = 20`, powers `[20, 10, 10]`, `total_voting_power = 40` — validator
1 ends up holding `20 / 40 = 50%` of actual voting power, far above
the intended 20% cap, because clamping the largest stake shrank the
total the cap was computed against.

Three candidates were weighed: **iterative re-cap until stable**
(selected), an exact analytic fixed point (correct but delicate
integer-division/rounding-direction reasoning at the capped/uncapped
boundary — more room for a subtle off-by-one despite being "exact"),
and the single-pass clamp above (rejected — demonstrably wrong, not a
real candidate).

Algorithm, given raw bonded stake `stake_i` per active validator and a
rational `cap_fraction = cap_numerator / cap_denominator` (both still
open — Open Decisions, below):

```text
total_0 = sum(stake_i)
for round r = 0, 1, 2, ...:
    C_r = floor(cap_numerator * total_r / cap_denominator)
    power_i = min(stake_i, C_r)           for every validator i
    total_(r+1) = sum(power_i)
    if total_(r+1) == total_r:
        voting_power(i) = power_i for every i
        total_voting_power = total_(r+1)
        stop
    total_r = total_(r+1)
```

`floor` division only — no floating point, consistent with this
ADR's own "Floating-Point Voting Power" rejection.

**Termination, deterministically, in at most `n` rounds** (`n` =
active validator count): `C_r` is non-increasing in `r`, because it is
computed from `total_r`, which is itself non-increasing (each
validator's power can only shrink or stay equal from one round to the
next). Therefore the set of validators with `stake_i > C_r` — the
"currently capped" set — can only grow from one round to the next,
never shrink. A monotonically growing subset of `n` validators has at
most `n + 1` distinct states, so the capped set (and with it `C_r` and
`total_r`) must stabilize within `n` rounds. The algorithm is also
order-independent (every round recomputes every validator's power from
`stake_i` and the current `C_r` directly — no sorting, no
tie-breaking rule needed, unlike leader election's `proposer_priority`
ties).

`cap_numerator`/`cap_denominator`'s concrete values, voting power's
integer width, and general rounding/overflow behavior for values
*other than* `C_r` itself remain open (Open Decisions, below) — same
scoping already used throughout this ADR: the mechanism is decided,
its parameters are not.

### Epoch Boundaries

Validator set changes should occur at deterministic boundaries.

Epoch-based activation is the preferred direction because it makes light-client
verification, checkpointing, and consensus safety easier to reason about.

The exact epoch length is open.

**Decided: epoch transition mechanism**, not its length. Epoch
boundaries are height-aligned: epoch `N` covers a fixed, contiguous
range of block heights (the exact `EPOCH_LENGTH` stays open — a
tunable constant, the same class of decision as ADR-0006's
`MAX_TRANSACTION_SIZE`, not a structural one). The active validator set
for epoch `N + 1` is derived from canonical state as it stands at the
*start* of epoch `N` — one full epoch of lead time before it takes
effect — giving validators and light clients an entire epoch to
receive, verify, and prepare for the next set rather than learning it
at the last possible height. New consensus keys become valid starting
at epoch `N + 1`'s first height, exactly; old keys remain valid for
verifying evidence and historical blocks from epoch `N` and earlier
(already stated conceptually in "Key Rotation," below — this decision
fixes the boundary precisely rather than leaving "remain valid" open).
This mechanism was decided independently of the voting power model and
the exact epoch length, both open at the time — both have since been
at least partly resolved (model: capped stake-weighted; epoch length's
constant remains open), and this mechanism needed no revision either
way, confirming the independence held.

### Validator Set Commitment

Blocks must commit to validator set or validator set transition data through
consensus metadata.

The commitment must be sufficient for:

- full node verification
- light-client finality verification
- checkpoint verification
- historical audit

**Decided: `validators_root` and `validator_set_commitment` /
`consensus_root` mechanism.**

```text
validators_root = MTH(active_validators, by ascending validator_id)
  where each leaf is
    HASH_PROFILE_0x0001("hnchain.validator.record.v1", HNCS(ValidatorRecordV1))

validator_set_commitment = HASH_PROFILE_0x0001(
  "hnchain.consensus.root.v1", HNCS(ValidatorSetCommitmentV1))
```

`MTH` is `hn-list-merkle-v1` (ADR-0008, "Ordered List Commitment") —
reused here for the same reason it was designed generically: any
ordered list of domain-separated digests can use it, not just
`transactions_root`/`receipts_root`. Leaves are sorted by ascending
`validator_id` — a canonical, content-independent order, avoiding an
"inclusion order" ambiguity that would need its own separate rule to
define.

**`validator_set_commitment` (used throughout ADR-0012's votes and
quorum certificates, ADR-0015's evidence, and here) and `consensus_root`
(ADR-0008's `BlockHeader` field) are the same value, not two
independently-computed digests that happen to agree.** A block's
`consensus_root` *is* the active validator set's
`validator_set_commitment` — this is what lets a light client verify
that the votes referenced by a block's `justification` actually used
the validator set the block itself claims, by comparing one field
against the other directly, rather than needing to trust that two
separately-named fields were computed consistently.

`ValidatorSetCommitmentV1`'s conceptual `hash_profile` field
(`validator-set.md` §7) is dropped: every hash in this protocol already
uses `HASH_PROFILE_0x0001` with an unambiguous domain tag, so a field
naming which profile was used would duplicate what the domain tag
`hnchain.consensus.root.v1` already guarantees — the same redundancy
class as `protocol_name` inside `TransactionSigningPayload` (ADR-0006)
and `checksum_profile` inside `AddressPayload` (ADR-0003 Decision 5).

### Key Rotation

Consensus key rotation must be explicit and delayed.

Immediate key replacement is rejected because it can create ambiguity in
in-flight consensus messages.

Key rotation must define:

- authorization
- activation epoch
- old key validity window
- evidence implications
- light-client verification behavior

### Slashing And Penalties

This ADR does not activate slashing.

If slashing is activated later, penalty rules must be deterministic and based on
canonical evidence.

Validator lifecycle must support non-slashing penalties such as jailing,
temporary inactivity, and reward reduction.

### Metadata

Validator metadata is not consensus authority.

Human-readable metadata may include operator name, website, region, or policy
documents, but consensus must commit only to bounded metadata hashes or
versioned metadata records.

## Rejected Options

### Fixed Validator Count Without Analysis

Rejected because active set size affects decentralization, finality latency,
bandwidth, quorum verification cost, and attack surface.

### One Coin Equals One Vote As Default

Rejected because direct stake weight without caps or delegation design can
increase governance and consensus concentration risk.

Stake-weighted voting remains a candidate, but it requires centralization
analysis and economic modeling.

### Validator Identity As IP Address

Rejected because IP addresses change, can be shared, and are not stable
cryptographic identities.

### Immediate Validator Set Changes

Rejected because changing voting power in the middle of consensus can create
safety and light-client ambiguity.

### Floating-Point Voting Power

Rejected because consensus arithmetic must be deterministic across
implementations and platforms.

## Alternatives Considered

### Equal Weight Active Validators

Advantages:

- simple quorum calculation
- reduces direct stake concentration inside consensus
- easy to test and explain

Disadvantages:

- needs a Sybil-resistance mechanism outside voting weight
- may underprice high-economic-stake validators
- active set admission becomes politically and economically sensitive

Not chosen (Decision, above): this project's architecture already
anticipates stake and delegation as real concepts (`stake`/`unstake`
transaction types, ADR-0006; the `validators` domain already holding
per-validator/delegator records, ADR-0007) — discarding economic
weighting entirely would make that existing structure largely
pointless for consensus purposes.

### Stake-Weighted Validators

Advantages:

- aligns voting influence with bonded economic exposure
- widely used and familiar in proof-of-stake systems
- simpler incentive mapping

Disadvantages:

- concentration risk
- delegation markets can centralize
- large holders may dominate consensus

Not chosen in its plain form (Decision, above; Rejected Options, "One
Coin Equals One Vote As Default"): the concentration risk here is
exactly what this ADR's own Rejected Options section already refuses
to accept as a default without further analysis. Capped stake weight,
below, keeps this option's economic alignment while directly
addressing its main documented risk.

### Capped Stake Weight

Advantages:

- preserves some economic weighting
- limits maximum influence of one validator
- can improve decentralization under concentrated stake

Disadvantages:

- may encourage stake splitting
- requires Sybil-resistance and delegation rules
- more complex economics

**Selected** (Decision, above). The additional complexity this option's
own disadvantages name — cap parameters, Sybil-resistance, delegation
rules — is exactly what stays open (Open Decisions, below); choosing
this model does not resolve them, only which direction they get
resolved within. The capping algorithm's mechanism (iterative re-cap
until stable, not the naive single-pass clamp) is now decided in
"Voting Power," above — the exact `cap_fraction` value remains open.

### Committee-Based Active Set

Advantages:

- can reduce per-block voting overhead
- may support large validator populations
- useful for scaling finality

Disadvantages:

- committee selection randomness becomes critical
- more complex light-client proofs
- additional liveness and censorship risks

**Ruled out** (Decision, above): "committee selection randomness
becomes critical" directly conflicts with ADR-0011's already-decided
"deterministic weighted round-robin, no randomness source at all." Not
rejected on its own merits — excluded by an already-made decision in a
different ADR.

## Security Considerations

Stake centralization:

- Risk: a small group controls quorum.
- Mitigation: active set policy, caps, delegation design, monitoring, and
  governance constraints.

Sybil validators:

- Risk: one operator appears as many validators.
- Mitigation: bonding, operational requirements, identity separation, and
  admission rules.

Key compromise:

- Risk: attacker signs consensus messages using validator keys.
- Mitigation: key rotation, key separation, hardware signing, evidence rules,
  and jailing.

Ambiguous validator set:

- Risk: nodes disagree on who may vote.
- Mitigation: deterministic epoch transitions and validator set commitments.

Long-range attack:

- Risk: old validator keys sign alternative history.
- Mitigation: unbonding windows, checkpoints, light-client rules, and historical
  set verification.

Network centralization:

- Risk: only operators with high-end infrastructure can remain active.
- Mitigation: measured requirements, relay non-dependence, performance profiles,
  and conservative active set targets.

Delegation capture:

- Risk: users delegate to a few large operators.
- Mitigation: delegation UX, caps, reward curves, and transparent validator
  metrics.

## Compatibility

Changing validator set derivation, voting power calculation, or epoch transition
rules is a major protocol change.

Adding new validator metadata can be backward-compatible only if:

- the metadata field is versioned
- size limits are defined
- old nodes can ignore it without changing consensus
- activation rules are explicit

Changing consensus key algorithms requires cryptographic identity migration
rules and light-client compatibility analysis.

## Open Decisions

Every economic-parameter item below (cap value, bond, epoch length,
unbonding period) is owned by ADR-0023 (Tokenomics And Economic Model),
not this ADR — this ADR decides the mechanism each one plugs into, never
the value itself. Several are now resolved there.

- `MAX_ACTIVE_SET_SIZE` value (`K`) — **resolved, ADR-0023: `K = 100`**
  (derivation mechanism decided above — bounded, top-K by
  `voting_power` descending)
- voting power's *maximum value bound* and zero-power behavior (model,
  capping algorithm, integer type, overflow behavior, and rounding
  behavior all decided above — `u128`, checked arithmetic, floor only;
  a bound below `u128::MAX` and what zero stake/power means for active
  set membership are the remaining narrower questions — ADR-0023, still
  open)
- minimum validator bond — still open (ADR-0023: deliberately not
  fixed to an absolute HNCOIN figure without a stable price estimate;
  may end up denominated some other way, e.g. as a share of supply)
- delegation support — **resolved, ADR-0023: supported from genesis**;
  stake concentration limits still open (no dominant industry practice
  yet)
- stake caps — **resolved, ADR-0023: `cap_numerator/cap_denominator =
  1/10`** (no single validator may exceed 10% of a round's voting
  power; capping algorithm mechanism decided above)
- epoch length — still open (transition mechanism decided above, only
  the constant remains); no longer blocked on `block_time`, which
  ADR-0009's own "Decided: Target Block Time" (2 seconds) has since
  resolved
- key rotation delay (ADR-0023, still open)
- unbonding period — **resolved, ADR-0023: 21 days, `907_200` blocks**
  (distinct from activation/deactivation, which are decided as a
  mechanism above — "Decided: admission mechanism"). The fund-release
  implementation is also done:
  [`hn_state::apply_unstake`](../../crates/hn-state/src/validator_transition.rs)
  records a `PendingUnbondingV1` maturing `UNBONDING_PERIOD_BLOCKS`
  later;
  [`hn_state::apply_unbonding_release`](../../crates/hn-state/src/validator_transition.rs)
  credits it back once matured — not yet invoked by anything, since no
  block-processing pipeline exists yet to call it automatically
  (`hn-consensus`/`hn-node` are still stubs).
- jail duration / release condition — **resolved, ADR-0015: no
  duration at all.** `validator_activate` is valid directly from
  `Jailed` (not only `Candidate`/`Inactive`) — see ADR-0015's own
  "Reactivation" text, updated.
- slashing activation — **resolved, ADR-0023: not activated.**
  Jailing (already decided, ADR-0015) stays the only active
  accountability mechanism; monetary slashing amounts remain deferred.
- validator metadata schema
- light-client validator set proof format
- hardware and bandwidth requirements

## Related Specifications

- `docs/rfc/consensus/validator-set.md`
- `docs/rfc/consensus/leader-selection.md`
