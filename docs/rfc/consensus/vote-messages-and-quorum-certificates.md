# HNChain Consensus RFC: Vote Messages And Quorum Certificates

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
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0013-finality-rules.md`

## 1. Purpose

This RFC defines the conceptual vote message and quorum certificate model for
HNChain consensus.

It specifies required fields, signing context, certificate verification,
threshold accounting, evidence hooks, and light-client requirements.

## 2. Scope

This RFC defines:

- vote message structure
- quorum certificate structure
- vote context binding
- validator eligibility checks
- signer uniqueness requirements
- voting power accounting requirements
- aggregation requirements
- evidence compatibility requirements

This RFC does not define:

- final consensus protocol
- final active vote type registry
- final quorum threshold formula
- final signature aggregation scheme
- final slashing penalties
- final checkpoint interval

## 3. Vote Message

Conceptual structure:

```text
ConsensusVoteV1
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

All consensus fields must be HNCS-encoded.

## 4. Vote Field Semantics

### 4.1 Vote Version

`vote_version` identifies the vote schema.

Unknown versions are rejected unless upgrade rules define acceptance.

### 4.2 Consensus Profile

`consensus_profile` identifies the active consensus rules.

Votes from another profile are invalid in the current profile.

### 4.3 Vote Type

`vote_type` identifies the consensus claim.

Candidate vote types:

```text
proposal
prevote
precommit
commit
timeout
nil
checkpoint
```

The final registry is defined by the accepted consensus profile.

### 4.4 Height, Round, And Epoch

`height`, `round`, and `epoch` bind the vote to one consensus context.

They must match the active block and validator set rules.

### 4.5 Validator Set Commitment

`validator_set_commitment` identifies the validator set used to validate voting
power and signer eligibility.

### 4.6 Validator ID

`validator_id` identifies the validator signing the vote.

The validator must be active and eligible in the referenced set.

### 4.7 Target Type And Target Hash

`target_type` defines what is being voted on.

Candidate target types:

```text
block
proposal
checkpoint
timeout
nil
```

`target_hash` is the canonical hash of the target object, or the specified
empty target value for `nil`.

### 4.8 Vote Metadata

`vote_metadata` is bounded, versioned, and profile-specific.

It must not contain local timing, debug logs, or network packet metadata.

### 4.9 Signature

The signature uses the validator consensus key and cryptographic identity rules.

The signing payload must exclude the signature field itself.

## 5. Vote Signing Payload

Conceptual signing payload:

```text
VoteSigningPayloadV1
  protocol_name
  chain_id
  network_id
  consensus_profile
  vote_version
  vote_type
  epoch
  height
  round
  validator_set_commitment
  validator_id
  target_type
  target_hash
  vote_metadata
  signing_purpose
```

The payload is encoded using HNCS and hashed using a vote signing hash profile.

## 6. Quorum Certificate

Conceptual structure:

```text
QuorumCertificateV1
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

The certificate proves that sufficient voting power signed the same target in
the same consensus context.

## 7. Certificate Verification

Verification requires:

- supported `qc_version`
- supported `consensus_profile`
- known validator set commitment
- valid certificate type
- canonical target type and hash
- valid signer commitment
- no duplicate signers
- every signer eligible
- correct voting power total
- signed voting power meets threshold
- aggregate proof or individual signatures verify

Verification must be deterministic and bounded.

## 8. Signer Commitment

The signer commitment identifies which validators signed.

Candidate representations:

```text
bitmap
sorted_validator_ids
merkle_signer_root
aggregate_signature_metadata
```

The accepted representation must define canonical ordering and malformed input
rejection.

## 9. Aggregation Profiles

Final aggregation profiles are open.

Candidate profiles:

```text
individual_signatures
batch_verified_signatures
bls_aggregate
threshold_signature
```

Every aggregation profile must define:

- signature algorithm compatibility
- key registration requirements
- signer proof format
- verification algorithm
- failure behavior
- test vectors

## 10. Evidence Compatibility

Votes must be usable as evidence for equivocation or conflicting consensus
claims.

Evidence verification must have access to:

- canonical vote bytes
- validator identity
- validator set commitment
- target type and hash
- vote type
- height, round, and epoch
- signature

## 11. Block Format Interaction

Quorum certificates may appear in block `justification`, `consensus_root`, or
checkpoint data depending on the accepted consensus profile.

The certificate must bind to the block hash when used as finality proof for a
block.

## 12. Light-Client Requirements

Light clients must be able to verify:

- validator set commitment
- quorum threshold
- signer eligibility
- signed voting power
- target hash binding
- certificate signature proof

If a light client cannot verify these fields, the certificate is not a
light-client finality proof.

## 13. Security Requirements

Implementations must reject:

- unknown vote versions
- unknown certificate versions
- malformed canonical encodings
- signatures with missing context
- votes for another chain or network
- votes for another height, round, or epoch
- duplicate signers in a certificate
- ineligible signers
- incorrect voting power sums
- certificates below threshold
- aggregate proofs with unsupported algorithms
- certificates with mismatched target hashes
- oversized vote metadata
- oversized certificates

Implementations must bound:

- vote decode cost
- vote verification cost
- certificate decode cost
- certificate verification cost
- signer set memory
- aggregate proof verification time

## 14. Test Vectors

The accepted version must include test vectors for:

- single valid vote
- vote replay rejection
- wrong chain rejection
- wrong round rejection
- inactive validator rejection
- duplicate signer rejection
- below-threshold certificate rejection
- valid quorum certificate
- malformed signer commitment rejection
- conflicting vote evidence
- aggregate proof rejection

Test vectors are mandatory before production implementation.

## 15. Open Decisions

- final vote type registry
- final target type registry
- final quorum threshold formula
- final quorum certificate schema
- final signer commitment format
- final aggregation profile
- final vote signing hash profile
- final evidence conflict rules
- final light-client proof format
- final maximum vote size
- final maximum certificate size
