# ADR-0012: Vote Messages And Quorum Certificates

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0011: Leader Election

Supersedes: None

## Context

Consensus votes are signed validator statements about proposals, rounds,
timeouts, finality, or other consensus claims.

Quorum certificates aggregate enough valid votes to prove that a consensus
threshold was reached under the active validator set.

HNChain block `justification` cannot be specified safely until vote messages,
vote signing context, quorum thresholds, duplicate handling, and certificate
verification are defined.

## Decision

HNChain defines versioned consensus vote messages and versioned quorum
certificates.

Conceptual vote:

```text
ConsensusVote
  vote_version
  consensus_profile
  vote_type
  chain_id
  network_id
  epoch
  height
  round
  validator_set_commitment
  validator_id
  target_type
  target_hash
  vote_metadata
  signature
```

Conceptual quorum certificate:

```text
QuorumCertificate
  qc_version
  consensus_profile
  certificate_type
  chain_id
  network_id
  epoch
  height
  round
  validator_set_commitment
  target_type
  target_hash
  quorum_threshold
  total_voting_power
  signed_voting_power
  signer_commitment
  aggregate_proof
```

The final signature aggregation scheme is open. Candidate approaches include
individual signatures with bitmaps, batch verification, BLS aggregation, and
threshold signatures.

## Normative Rules

### Versioned Vote Messages

Every vote includes `vote_version`.

Nodes must not infer vote format from signature length, public key algorithm,
network message type, or consensus client version.

### Vote Context Binding

Every vote signature must bind to:

- protocol name
- chain ID
- network ID
- consensus profile
- vote version
- vote type
- epoch
- height
- round
- validator set commitment
- validator ID
- target hash
- signing purpose

Votes valid in one context must not be replayable in another context.

### Vote Types

Vote types are consensus-profile specific.

Initial conceptual vote classes:

- `proposal`
- `prevote`
- `precommit`
- `commit`
- `timeout`
- `nil`
- `checkpoint`

The accepted consensus profile must define which vote types are active.

### Validator Eligibility

A vote is valid only if:

- validator is in the active validator set for the vote context
- validator has nonzero voting power
- validator consensus key is valid for the epoch
- validator status permits voting
- signature verifies under the validator's consensus key

### Duplicate And Conflicting Votes

Duplicate votes from the same validator for the same target are ignored after
the first valid vote.

Conflicting votes from the same validator for the same height, round, vote type,
and safety domain are evidence candidates.

The exact conflict rules must be defined by the consensus profile.

### Quorum Threshold

Quorum calculation must use integer arithmetic.

The threshold must be defined over total voting power for the active validator
set.

For BFT profiles targeting fewer than one third Byzantine voting power, the
expected direction is a threshold greater than two thirds of total voting power.

The exact formula remains open until the final consensus profile is accepted.

### Quorum Certificate Target

A quorum certificate must bind to a single target.

The target may be:

- block hash
- proposal hash
- checkpoint hash
- timeout claim hash
- consensus object hash

The target type must be explicit.

### Signer Commitment

The certificate must commit to the signer set.

The commitment may be represented by a canonical bitmap, sorted signer list,
Merkle root, aggregate signature metadata, or another specified format.

The final representation must support deterministic verification and malformed
signer rejection.

### Aggregation

Signature aggregation is an optimization, not a hidden consensus rule.

The certificate must define enough data to verify:

- signer eligibility
- signer uniqueness
- signed voting power
- target binding
- signature validity
- threshold satisfaction

### Light-Client Verification

Quorum certificates must be verifiable by light clients with the validator set
commitment and required validator set proof.

Light clients must not trust RPC assertions that a block is finalized without
verifying the certificate or an accepted checkpoint proof.

### Evidence

Votes must be canonical enough to serve as evidence.

Evidence validity must not depend on local logs, mempool state, network arrival
order, or operator testimony.

## Rejected Options

### Unversioned Votes

Rejected because consensus message formats must evolve over decades without
ambiguous parsing.

### Vote Signatures Without Context

Rejected because signatures could be replayed across vote types, rounds,
heights, chains, or networks.

### Floating-Point Quorum Calculation

Rejected because consensus arithmetic must be deterministic across platforms.

### Quorum Certificate Without Signer Set

Rejected because nodes must verify signer uniqueness and voting power.

### RPC Finality Flag As Consensus Proof

Rejected because RPC responses are not consensus objects.

## Alternatives Considered

### Individual Signatures With Bitmap

Advantages:

- simple to reason about
- no special aggregation cryptography
- strong compatibility with algorithm agility

Disadvantages:

- larger certificates
- more verification work
- less efficient for large validator sets

### Batch Verification

Advantages:

- keeps individual signatures
- can improve verification performance
- simpler than aggregate-signature consensus

Disadvantages:

- still large on the wire
- batch failure handling must be deterministic
- algorithm support varies

### BLS Aggregate Signatures

Advantages:

- compact certificates
- efficient light-client proofs
- good fit for large validator sets

Disadvantages:

- adds pairing-based cryptographic assumptions
- requires careful rogue-key protection
- post-quantum migration needs separate analysis

### Threshold Signatures

Advantages:

- compact finality proof
- can hide signer set details if designed that way

Disadvantages:

- complex distributed key management
- harder validator churn
- difficult accountability unless signer evidence is preserved

## Security Considerations

Replay attacks:

- Risk: a valid vote is reused in another context.
- Mitigation: mandatory signing context and domain separation.

Equivocation:

- Risk: validator signs conflicting votes.
- Mitigation: canonical vote evidence and deterministic conflict rules.

Quorum inflation:

- Risk: duplicate validators or malformed signer sets inflate voting power.
- Mitigation: canonical signer commitment, duplicate rejection, and validator
  set commitment verification.

Rogue-key attacks:

- Risk: aggregate signature schemes are abused by malicious key registration.
- Mitigation: proof-of-possession or scheme-specific registration rules if BLS
  or similar aggregation is selected.

Certificate bloat:

- Risk: large certificates harm block propagation and light-client usability.
- Mitigation: size limits, aggregation evaluation, and benchmarked formats.

Long-range attacks:

- Risk: old validator keys sign alternative certificates.
- Mitigation: validator set history, unbonding windows, checkpoints, and
  light-client security rules.

Ambiguous finality:

- Risk: different clients interpret certificate types differently.
- Mitigation: explicit certificate type, target type, and profile version.

## Compatibility

Changing vote signing context, quorum threshold, certificate target semantics,
or aggregation scheme is a major consensus change.

Adding a vote type can be compatible only if:

- vote type is registered
- signing context is defined
- unsupported nodes reject it deterministically
- activation rules are explicit

Changing signature aggregation requires cryptographic identity and light-client
compatibility analysis.

## Open Decisions

- initial vote types
- final quorum threshold formula
- final quorum certificate format
- signer commitment representation
- signature aggregation scheme
- batch verification rules
- evidence conflict rules
- nil vote semantics
- timeout certificate semantics
- checkpoint certificate semantics
- light-client validator set proof format
- maximum vote size
- maximum certificate size
- certificate inclusion in block justification

## Related Specifications

- `docs/rfc/consensus/vote-messages-and-quorum-certificates.md`
