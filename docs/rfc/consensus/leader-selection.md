# HNChain Consensus RFC: Leader Selection

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0011-leader-election.md`

## 1. Purpose

This RFC defines the conceptual leader selection interface for HNChain
consensus.

It specifies required inputs, outputs, verification rules, fallback behavior,
and security constraints for any accepted leader election profile.

## 2. Scope

This RFC defines:

- election input requirements
- proposer output requirements
- validator set binding
- randomness and rotation requirements
- lookahead constraints
- proposer proof requirements
- failure handling requirements

This RFC does not define:

- final consensus algorithm
- final randomness beacon
- final VRF algorithm
- final round timeout values
- final proposer reward rules
- final committee selection algorithm

## 3. Election Input

Conceptual structure:

```text
LeaderElectionInputV1
  election_version
  consensus_profile
  chain_id
  network_id
  epoch
  height
  round
  validator_set_commitment
  randomness_commitment
  protocol_parameters_hash
```

All inputs must be canonical consensus values.

## 4. Election Output

Conceptual structure:

```text
LeaderElectionOutputV1
  proposer
  proof
  priority_metadata
```

`proposer` is the canonical validator identity selected for the target height
and round.

`proof` is optional only for election profiles where selection is fully
deterministic from public inputs.

`priority_metadata` is profile-specific and must be bounded.

## 5. Validator Set Binding

Leader selection operates on the active validator set for the relevant epoch.

The election profile must reject:

- inactive validators
- jailed validators
- exited validators
- validators with zero voting power
- validators with invalid consensus keys
- validators outside the committed active set

## 6. Election Profiles

Final profiles are open.

Candidate profile families:

```text
round_robin
weighted_round_robin
vrf_weighted
random_beacon_weighted
committee_proposer
```

Every profile must define:

- input schema
- output schema
- verification algorithm
- proposer probability model
- fallback behavior
- test vectors

## 7. Randomness Requirements

If an election profile uses randomness, the randomness must be:

- protocol-defined
- domain-separated
- committed in consensus metadata
- verifiable by full nodes
- verifiable by light clients when needed
- resistant to practical bias under stated assumptions
- bounded in decode and verification cost

Randomness must not depend on:

- local OS entropy
- local clock time
- RPC response order
- network packet arrival order
- mempool contents

## 8. Lookahead

Lookahead defines how early a proposer can be known.

The accepted profile must define:

- public lookahead window
- private eligibility window, if VRF-style selection is used
- operational preparation assumptions
- targeted attack risk
- behavior across epoch boundaries

## 9. Fallback Behavior

When a proposer fails, the consensus profile must define how the next proposer
is selected.

Fallback rules must bind to:

- height
- round
- epoch
- validator set commitment
- timeout evidence or round-change proof, if required

Fallback must be deterministic for all honest validators.

## 10. Proposer Proof

When required, proposer proof must bind to:

```text
chain_id
network_id
consensus_profile
election_profile
validator_id
epoch
height
round
validator_set_commitment
randomness_commitment
```

Proof verification must be bounded.

## 11. Block Format Interaction

Leader selection verifies or produces the block header `proposer` field.

Related block fields:

```text
height
round
epoch
proposer
consensus_root
protocol_parameters_hash
```

Leader selection must not depend on block body transaction ordering.

## 12. Security Requirements

Implementations must reject:

- unknown election profiles
- proposer outside the active validator set
- proposer with invalid status
- proposer with invalid consensus key
- malformed proposer proof
- proposer proof for another chain or network
- proposer proof for another height, round, or epoch
- randomness commitments with unsupported hash profiles
- election outputs with non-canonical encoding

Implementations must bound:

- election verification time
- proposer proof size
- randomness proof size
- leader schedule cache size
- fallback round search

## 13. Test Vectors

The accepted version must include test vectors for:

- single-validator selection
- multi-validator deterministic selection
- inactive validator rejection
- jailed validator rejection
- zero-power validator rejection
- epoch-boundary selection
- round fallback selection
- invalid randomness commitment rejection
- invalid proposer proof rejection
- tie-breaking

Test vectors are mandatory before production implementation.

## 14. Open Decisions

- final election profile
- final randomness source
- final VRF or beacon scheme
- final proposer proof format
- final lookahead window
- final fallback behavior
- final tie-breaking rules
- final interaction with committee selection
- final light-client verification rules
- final benchmark and simulation requirements
