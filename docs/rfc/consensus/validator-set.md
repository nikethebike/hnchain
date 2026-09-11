# HNChain Consensus RFC: Validator Set

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0001-account-state-model.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0011-leader-election.md`

## 1. Purpose

This RFC defines the conceptual validator set model for HNChain consensus.

It specifies the state objects, lifecycle, voting power constraints, epoch
transition boundaries, and verification requirements that future consensus
profiles must use.

## 2. Scope

This RFC defines:

- validator record structure
- validator lifecycle
- active set derivation requirements
- voting power requirements
- validator set commitment requirements
- key rotation requirements
- compatibility and security requirements

This RFC does not define:

- final staking economics
- final reward distribution
- final slashing amounts
- final leader election algorithm
- final quorum certificate format
- final active set size

## 3. Validator Record

Conceptual structure:

```text
ValidatorRecordV1
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

**Decided** (ADR-0010, "Decided: `validator_id` derivation" /
"Decided: `bonded_stake`, distinct from `voting_power`"): `account_address`
is dropped — `validator_id` is the controlling account's own
`address_body`, so a separate field would duplicate it. `bonded_stake`
is added (raw bonded stake, `u128`) — distinct from `voting_power`
(capped, already decided): `stake`/`unstake` mutate `bonded_stake`
directly; `voting_power` only changes when the capping algorithm
recomputes it over the full candidate set. See §4.2.

All fields that affect consensus must be HNCS-encoded and committed through the
state tree.

## 4. Field Semantics

### 4.1 Record Version

`record_version` identifies the validator record schema.

Unknown versions are rejected unless protocol upgrade rules define acceptance.

### 4.2 Validator ID

`validator_id` is a stable protocol identifier for a validator record.

It must not be an IP address, DNS name, display name, or implementation-specific
database key.

**Decided** (ADR-0010, "Decided: `validator_id` derivation"):
`validator_id` is the controlling account's own `address_body` (ADR-0003,
`account` namespace) — reuses `hn_crypto::account_address_body`'s
existing output, no new derivation function. Stays stable across
consensus key rotation, since it never depends on `consensus_key` at
all, unlike `hn_crypto::validator_address_body` (ADR-0003, `validator`
namespace), which *is* key-derived and rotates with it. This also
settles authorization for validator-management transactions: for a
self-managed validator, `sender == validator_id` directly, no separate
ownership field needed.

### 4.3 Account Address

`account_address` links the validator record to the account that owns or
controls validator permissions.

**Decided** (ADR-0010, "Decided: `validator_id` derivation"): this
field is dropped, not merely resolved — `validator_id` (§4.2) *is* the
controlling account's address now, so a separate `account_address`
field would duplicate it (kept here only as a historical/conceptual
note; not part of the decided `ValidatorRecordV1` field list, §3).

### 4.4 Consensus Key

`consensus_key` signs consensus messages.

The key descriptor must include algorithm, key version, lifecycle state, and
verification context as defined by cryptographic identity specifications.

### 4.5 Network Key

`network_key` identifies or authenticates the validator node at the networking
layer when required.

Consensus validity must not depend on unauthenticated network identity strings.

### 4.6 Status

`status` is one of the accepted validator lifecycle states.

Initial conceptual registry:

```text
registered
candidate
active
inactive
jailed
exited
```

### 4.7 Voting Power

`voting_power` is an unsigned integer consensus value.

**Decided** (ADR-0010, "Decided: voting power integer type — `u128`"):
matches `BalanceValueV1.native_balance`'s own width — the capping
algorithm (§8) operates directly on bonded stake with no rescaling
step, so voting power shares stake's unit. A maximum value bound below
`u128::MAX`, if any, remains open.

**Decided** (ADR-0010, "Decided: `bonded_stake`, distinct from
`voting_power`"): "bonded stake" here is now a named field,
`bonded_stake` (§3), not just this section's own conceptual input —
`stake`/`unstake` (ADR-0006) mutate it directly; `voting_power` changes
only when the capping algorithm recomputes it over the full candidate
set (for example at an epoch boundary), never synchronously with a
single validator's own stake change.

Voting power must not use floating-point arithmetic.

### 4.8 Activation And Deactivation Epochs

Activation and deactivation are delayed to deterministic boundaries.

The final delay rules are open.

### 4.9 Metadata Hash

`metadata_hash` commits to optional bounded validator metadata.

Metadata must not define consensus authority.

## 5. Lifecycle

Conceptual lifecycle:

```text
registered
  -> candidate
  -> active
  -> inactive
  -> exited
```

Penalty path:

```text
active
  -> jailed
  -> inactive
```

Lifecycle transitions require deterministic authorization and validation rules.

**Decided** (ADR-0010, "Decided: admission mechanism"): `registered →
candidate` is automatic (a derived condition — bonded stake meets the
still-open minimum bond — not its own transaction, since §11's
operation registry names no `validator_candidate` operation);
`candidate → active` is explicit, via `validator_activate`, so an
operator opts in rather than being drafted into consensus duty by a
balance check alone. Both `validator_activate` and `validator_deactivate`
take effect only at the next epoch boundary — the same one-epoch lead
time §9 already gives validator *set* transitions generally, applied
here per validator rather than to the aggregate set. Jailing (the
penalty path above) is untouched by this decision — it belongs to
ADR-0015 (Slashing And Accountability), still fully open.

## 6. Active Set Derivation

The active validator set for a height or epoch is derived from canonical state.

The final derivation function must define:

- input state root
- eligible statuses
- stake or bond requirements
- ranking rule
- tie-breaking rule
- maximum active set size, if any
- voting power calculation
- activation delay
- deactivation delay
- deterministic output ordering
- test vectors

**Decided** (ADR-0010, "Decided: active set derivation mechanism"):
eligible statuses = `active` only; input state / timing reuses "Epoch
Boundaries" (§9) — a height's set is whichever epoch's already-
snapshotted set that height falls under; deterministic output ordering
reuses `validators_root`'s ascending `validator_id` order (§7). Active
set shape is **bounded**: top-K by `voting_power` descending, ties by
ascending `validator_id` (reusing leader-election's own tie-break,
ADR-0011) — asked the user explicitly, since this is not mechanically
forced the way the other three items are. `MAX_ACTIVE_SET_SIZE` (`K`)
itself, stake/bond requirements, activation delay, and deactivation
delay remain open (§14).

Conceptual function, decided shape:

```text
ACTIVE_SET(epoch) -> Vec<ValidatorRecordV1>
  candidates = { v : v.status == active }
  ranked = candidates sorted by (voting_power desc, validator_id asc)
  selected = ranked.take(MAX_ACTIVE_SET_SIZE)
  ACTIVE_SET = selected, re-ordered ascending by validator_id
```

## 7. Validator Set Commitment

Every validator set used for consensus must be committed.

Conceptual structure:

```text
ValidatorSetCommitmentV1
  set_version
  epoch
  validator_count
  total_voting_power
  validators_root
  hash_profile
```

The final commitment format must support full nodes and light clients.

## 8. Voting Power Calculation

Voting power calculation must be deterministic.

It must define:

- source of stake or weight
- integer width
- maximum value
- total power limit
- rounding rules
- overflow behavior
- zero-power behavior
- quorum threshold interaction

If stake-weighted voting is selected, the model must analyze centralization
risk and delegation concentration before acceptance.

**Decided** (ADR-0010, "Decided: capped stake-weighted voting power"):
source of weight is bonded stake, capped at a maximum share of total
voting power — asked the user explicitly, given the weight of this
decision and this section's own analysis requirement. Committee-based
weighting is ruled out (needs randomness ADR-0011 already excludes from
leader election entirely). The cap's exact value remains open (integer
width decided separately — `u128`, see §4.7).

**Decided** (ADR-0010, "Decided: capping algorithm"): iterative re-cap
until stable — each round, clamp every validator's power at
`floor(cap_numerator * total / cap_denominator)`, recompute the total,
repeat until the total stops changing; terminates within `n` rounds
(`n` = active validator count) since the capped set only grows round
over round. Not a single-pass clamp: clamping the largest stakes
shrinks the total the cap was computed against, so a single pass does
not actually bound any validator's post-normalization share (worked
example in ADR-0010). `cap_numerator`/`cap_denominator`'s concrete
value remains open (integer width decided separately — `u128`, §4.7).

## 9. Epoch Transitions

Validator set changes should occur at epoch boundaries.

Epoch transition rules must define:

- when pending changes are sampled
- which state root determines the next set
- when new keys become valid
- when old keys stop being valid
- how light clients verify the transition
- what happens if a transition block is missing or delayed

## 10. Key Rotation

Key rotation requires an authenticated validator operation.

Conceptual flow:

```text
current_consensus_key
  -> rotation_request
  -> pending_key
  -> activation_epoch
  -> active_consensus_key
```

Old keys may remain valid for evidence verification and historical block
verification.

## 11. Validator Operations

Final transaction schemas are open.

Conceptual operation types:

```text
validator_register
validator_update_keys
validator_update_metadata
validator_bond
validator_unbond
validator_activate
validator_deactivate
validator_exit
```

Every operation must define authorization, state transition rules, fee behavior,
and failure semantics.

## 12. Security Requirements

Implementations must reject:

- duplicate validator IDs
- duplicate active consensus keys
- unsupported key algorithms
- invalid key lifecycle states
- invalid status transitions
- negative or overflowing voting power
- non-deterministic active set ordering
- validator set commitments with incorrect totals
- immediate key rotations that affect in-flight consensus
- metadata exceeding protocol limits

Implementations must bound:

- active set size
- validator record size
- metadata size
- active set derivation time
- validator set proof size
- key rotation queue size

## 13. Test Vectors

The accepted version must include test vectors for:

- empty validator set rejection
- single validator record encoding
- duplicate validator ID rejection
- duplicate consensus key rejection
- active set derivation
- tie-breaking
- voting power total
- epoch transition
- key rotation activation
- jailed validator exclusion
- validator set commitment

Test vectors are mandatory before production implementation.

## 14. Open Decisions

- final validator record schema (`validator_id` derivation and
  `bonded_stake`/`voting_power` split decided; `network_key`,
  `activation_epoch`/`deactivation_epoch`, `metadata_hash` remain
  unresolved — see §3)
- voting power's maximum value bound and cap fraction value (model,
  capping algorithm, and integer type — `u128` — decided; see §8)
- `MAX_ACTIVE_SET_SIZE` value (selection mechanism decided; see §6)
- final epoch length
- final key rotation delay
- final unbonding period (activation/deactivation delay decided as a
  mechanism — one epoch; see §5 — unbonding is distinct)
- final validator operation transaction schemas
- final validator set commitment format
- final light-client proof format
