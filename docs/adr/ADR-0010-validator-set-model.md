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
  account_address
  consensus_key
  network_key
  status
  voting_power
  activation_epoch
  deactivation_epoch
  metadata_hash
```

`consensus_key` signs consensus messages.

`network_key` authenticates peer-to-peer node communication when required.

Operational keys must be rotatable without changing account ownership.

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
Integer type/bounds and general rounding/overflow behavior remain
open — the quorum threshold calculation itself is already decided
independently of all of these (ADR-0012, "Decided: quorum threshold
formula": `signed * 3 > total * 2`, exact integer arithmetic, applies
unchanged regardless of voting power's width or source).

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
This mechanism is independent of the voting power model (still open,
Open Decisions) and of the exact epoch length: both apply unchanged
regardless of which is eventually chosen.

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

- initial active validator set size policy
- voting power integer type/bounds and general rounding/overflow
  behavior (model and capping algorithm mechanism decided above —
  capped stake-weighted, iterative re-cap until stable; these are the
  remaining representation-level parameters)
- minimum validator bond
- delegation support
- stake caps (`cap_numerator`/`cap_denominator` value and
  Sybil-resistance rules — capping algorithm mechanism decided above)
- validator admission ranking
- epoch length (transition mechanism decided above; the constant itself
  is not)
- activation delay
- deactivation delay
- key rotation delay
- unbonding period
- jailing conditions
- slashing activation
- validator metadata schema
- light-client validator set proof format
- hardware and bandwidth requirements

## Related Specifications

- `docs/rfc/consensus/validator-set.md`
- `docs/rfc/consensus/leader-selection.md`
