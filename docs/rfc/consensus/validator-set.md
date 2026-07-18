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
  account_address
  consensus_key
  network_key
  status
  voting_power
  activation_epoch
  deactivation_epoch
  metadata_hash
```

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

### 4.3 Account Address

`account_address` links the validator record to the account that owns or
controls validator permissions.

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

The final integer width is open.

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

Conceptual function:

```text
ACTIVE_SET(state_root, epoch) -> ValidatorSet
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

- final validator record schema
- final validator ID derivation
- final voting power integer width
- final voting power model
- final active set selection algorithm
- final maximum active set size
- final epoch length
- final activation delay
- final deactivation delay
- final key rotation delay
- final validator operation transaction schemas
- final validator set commitment format
- final light-client proof format
