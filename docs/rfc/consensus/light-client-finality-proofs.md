# HNChain Consensus RFC: Light-Client Finality Proofs

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0013-finality-rules.md`
- `docs/adr/ADR-0016-synchronization-checkpoints.md`
- `docs/adr/ADR-0017-light-client-finality-proofs.md`
- `docs/adr/ADR-0018-p2p-protocol-messages.md`

## 1. Purpose

This RFC defines the conceptual light-client finality proof model for HNChain.

It specifies the proof inputs, trusted starting point, finality verification,
validator set update verification, state root relationship, and security
requirements for constrained clients.

## 2. Scope

This RFC defines:

- light-client proof structure
- trusted starting point requirements
- finalized header verification
- validator set update requirements
- weak subjectivity hooks
- state root verification boundary
- proof size and verification limits

This RFC does not define:

- final light-client update algorithm
- final weak subjectivity period
- final state proof format
- final bridge policy
- final RPC method schema
- final SDK API

## 3. Proof Object

Conceptual structure:

```text
LightClientFinalityProofV1
  proof_version
  chain_id
  network_id
  consensus_profile
  trusted_header
  target_header
  finality_proof
  validator_set_proof
  validator_set_update
  proof_metadata
```

All fields that affect verification must be HNCS-encoded or canonical headers
defined by the block format specification.

## 4. Trusted Starting Point

A light client starts from one of:

```text
genesis_header
verified_checkpoint
recent_weak_subjectivity_checkpoint
previously_verified_header
```

The starting point must be explicit in client state.

## 5. Verification Algorithm

Conceptual verification:

```text
light_client_proof
  -> version check
  -> chain and network check
  -> trusted header check
  -> target header hash verification
  -> validator set proof verification
  -> validator set update verification
  -> finality proof verification
  -> freshness or weak-subjectivity check
  -> accept finalized header
```

The accepted consensus profile may add profile-specific checks, but must not
remove chain binding, validator set verification, or finality proof
verification.

## 6. Header Verification

The target header must be canonical.

The light client must verify:

- header version
- chain ID
- network ID
- height
- parent link, if relevant to update mode
- state root
- consensus root
- protocol parameters hash
- block hash

## 7. Validator Set Verification

Validator set verification must prove which validators were eligible to sign the
target finality proof.

The proof must define:

- validator set commitment
- signer eligibility
- validator set transition path
- voting power total
- quorum threshold
- supported key profiles

## 8. State Root Boundary

A light-client finality proof verifies that a state root is finalized.

It does not prove an account balance, contract value, validator status, or event
by itself.

Those claims require separate state or event proofs against the finalized root.

## 9. Weak Subjectivity

If required by the consensus profile, the light client must enforce weak
subjectivity policy.

The policy must define:

- maximum offline period
- trusted checkpoint age
- validator set churn assumptions
- warning behavior
- rejection behavior
- recovery procedure

## 10. RPC And SDK Boundary

RPC and SDK APIs may transport light-client proofs.

They must not reinterpret proof validity.

Clients must verify canonical proof bytes locally or inside a locally
controlled verifier.

## 11. Security Requirements

Implementations must reject:

- unknown proof versions
- unsupported consensus profiles
- proofs for another chain or network
- non-canonical headers
- invalid block hashes
- invalid finality proofs
- invalid validator set proofs
- unsupported cryptographic profiles
- stale proofs beyond policy
- oversized proofs
- proofs with mismatched state roots

Implementations must bound:

- proof decode cost
- validator set proof size
- finality proof verification time
- signature verification count
- metadata size
- memory usage

## 12. Test Vectors

The accepted version must include test vectors for:

- valid light-client finality proof
- wrong chain rejection
- wrong network rejection
- wrong target header rejection
- invalid finality proof rejection
- invalid validator set proof rejection
- stale proof rejection, if policy exists
- unsupported crypto profile rejection
- state root mismatch rejection
- oversized proof rejection

Test vectors are mandatory before production implementation.

## 13. Open Decisions

- final proof schema
- final validator set proof format
- final update algorithm
- final weak subjectivity period
- final freshness rules
- final maximum proof size
- final state proof relationship
- final RPC transport schema
- final SDK verification API
- final bridge verification recommendations
