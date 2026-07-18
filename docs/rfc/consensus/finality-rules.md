# HNChain Consensus RFC: Finality Rules

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0011-leader-election.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0013-finality-rules.md`

## 1. Purpose

This RFC defines the conceptual finality model for HNChain consensus.

It specifies what a finality proof must bind to, how nodes and light clients
must verify finality, and which decisions remain open for the final consensus
profile.

## 2. Scope

This RFC defines:

- finality proof structure
- finality verification inputs
- block hash binding
- validator set binding
- quorum certificate dependency
- application-visible finality states
- light-client finality requirements
- security requirements

This RFC does not define:

- final consensus algorithm
- final quorum threshold formula
- final timeout values
- final checkpoint interval
- final weak subjectivity period
- final bridge confirmation policy

## 3. Finality Proof

Conceptual structure:

```text
FinalityProofV1
  proof_version
  consensus_profile
  chain_id
  network_id
  epoch
  height
  round
  block_hash
  validator_set_commitment
  quorum_certificate
  finality_metadata
```

All fields are consensus-relevant unless a final profile explicitly marks a
field as non-consensus metadata.

## 4. Finality States

Clients must distinguish the following states:

```text
seen
mempool_accepted
proposed
certified
finalized
```

Meanings:

- `seen`: a node observed an object locally.
- `mempool_accepted`: local mempool policy accepted a transaction.
- `proposed`: a block was proposed by an eligible proposer.
- `certified`: a quorum certificate exists for a consensus claim.
- `finalized`: the accepted finality rule proves irreversible commitment under
  stated assumptions.

Only `finalized` is finality.

## 5. Verification Inputs

Finality verification requires:

- canonical block header
- block hash profile
- finality proof
- quorum certificate
- validator set commitment
- validator set proof or trusted local validator set state
- protocol parameters
- consensus profile

RPC responses are not verification inputs unless they carry canonical proof
bytes that the client verifies independently.

## 6. Verification Algorithm

Conceptual verification:

```text
finality_proof
  -> version check
  -> chain and network check
  -> consensus profile check
  -> block hash recomputation
  -> height, round, and epoch check
  -> validator set commitment verification
  -> quorum certificate verification
  -> finality rule verification
  -> finalized
```

The accepted consensus profile may add profile-specific steps, but must not
remove context binding or quorum verification.

## 7. Block Format Interaction

Finality proof binds to the block header hash.

Related block fields:

```text
height
round
epoch
parent_block_hash
state_root
consensus_root
evidence_root
justification
```

When stored in `justification`, the proof must verify the block identified by
the header hash.

## 8. Validator Set Transitions

Finality across epoch boundaries must define:

- which validator set signs the transition block
- which validator set signs the next block
- how the next validator set commitment is proven
- how light clients update trusted validator set state
- how delayed or failed transitions are handled

Ambiguous validator set transitions are invalid.

## 9. Checkpoints

Checkpoints summarize finalized history.

Checkpoint rules must define:

- checkpoint height
- checkpoint hash
- signing or finality proof
- validator set used
- weak subjectivity implications
- light-client update behavior
- archival verification behavior

Checkpoints are not a substitute for undefined finality rules.

## 10. Data Availability

The finality profile must define whether validators are allowed to vote before
block body data is available.

If a profile permits finality without body availability, it must explicitly
document the risk and recovery path.

The preferred direction is that validators verify required block data before
casting finality-relevant votes.

## 11. Application Guidance

Applications must not equate:

- transaction broadcast with inclusion
- inclusion with finality
- proposal with finality
- local node acceptance with network finality
- RPC status with proof verification

High-value workflows should verify finality proofs or rely on services that
publish verifiable proof material.

## 12. Security Requirements

Implementations must reject:

- unknown finality proof versions
- unsupported consensus profiles
- proofs for another chain or network
- proofs for another block hash
- proofs with mismatched height, round, or epoch
- quorum certificates below threshold
- quorum certificates for another target
- validator set commitments that do not match the proof
- malformed finality metadata
- oversized finality proofs

Implementations must bound:

- proof decode cost
- validator set proof size
- quorum certificate verification time
- finality metadata size
- checkpoint proof size

## 13. Test Vectors

The accepted version must include test vectors for:

- valid finality proof
- wrong block hash rejection
- wrong chain rejection
- wrong epoch rejection
- invalid validator set rejection
- below-threshold quorum rejection
- conflicting finalized block scenario
- epoch transition finality
- checkpoint verification
- malformed proof rejection
- light-client proof verification

Test vectors are mandatory before production implementation.

## 14. Open Decisions

- final finality profile
- final quorum certificate chain rule
- final finality proof schema
- final checkpoint schema
- final weak subjectivity period
- final light-client update algorithm
- final data availability requirement
- final timeout and nil-vote interaction
- final bridge finality recommendation
- final maximum finality proof size
