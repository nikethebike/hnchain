# ADR-0017: Light-Client Finality Proofs

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0008: Block Format
- ADR-0010: Validator Set Model
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0013: Finality Rules
- ADR-0016: Synchronization Checkpoints

Supersedes: None

## Context

Light clients verify chain state without downloading and executing the full
block history.

Wallets, mobile clients, browsers, bridges, explorers, and monitoring systems
need proof-backed access to finalized headers, state roots, validator set
updates, and checkpoints.

HNChain must not require light clients to trust RPC providers for finality.

## Decision

HNChain defines versioned light-client finality proofs.

Conceptual proof:

```text
LightClientFinalityProof
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

The proof lets a light client verify that a target block header is finalized
relative to a trusted starting point and accepted validator set transition
rules.

This ADR does not define the final light-client update algorithm or weak
subjectivity period.

## Normative Rules

### Versioned Proof

Every light-client proof includes `proof_version`.

Nodes and clients must not infer proof format from RPC method, payload length,
wallet version, bridge adapter, or file extension.

### Header-Only Verification

Light clients verify canonical block headers and proof material.

They do not execute full state transitions unless operating in a full or
verifying-client mode.

### Trusted Starting Point

A light client needs a trusted starting point.

Candidate starting points:

- genesis header
- verified checkpoint
- recent weak-subjectivity checkpoint
- locally stored previously verified header

The trust model must be explicit to users and applications.

### Finality Proof Verification

The target header must be verified through a finality proof defined by
ADR-0013.

RPC statements are not proof.

### Validator Set Verification

The proof must verify the validator set used by the finality proof.

Validator set updates must be deterministic, bounded, and tied to finalized
headers or checkpoints.

### Chain And Network Binding

Every proof must bind to chain ID and network ID.

A proof valid on one network must not verify on another network.

### Hash And Signature Profiles

Light-client proofs must include or reference required hash profiles,
cryptographic identity profiles, and signature verification rules.

Unsupported profiles are rejected.

### State Root Access

A light-client finality proof establishes finalized header and state root
validity.

Account, contract, asset, or validator state claims require separate state
proofs against the finalized state root.

### Proof Size Limits

Proof size and verification cost must be bounded.

This is especially important for wallets, browsers, mobile devices, bridges,
and embedded clients.

### Weak Subjectivity

If the consensus profile requires weak subjectivity, light-client proof rules
must define:

- maximum safe offline period
- trusted checkpoint age
- validator set churn assumptions
- warning behavior
- recovery behavior

## Rejected Options

### Trust RPC Provider Finality

Rejected because RPC providers are not consensus authorities.

### Latest Block Hash Without Proof

Rejected because a block hash alone does not prove finality.

### Header Chain Without Validator Set Updates

Rejected because finality proof verification depends on knowing the correct
validator set.

### Hardcoded Validator Set Forever

Rejected because validator sets must evolve over time.

### Unbounded Proofs

Rejected because light clients require predictable verification resources.

## Alternatives Considered

### Full Verification On Client

Advantages:

- strongest independent verification
- simplest trust argument

Disadvantages:

- too expensive for many wallets and browsers
- not practical for low-resource devices

### Checkpoint-Based Light Client

Advantages:

- compact
- practical for wallets and bridges
- fits weak-subjectivity assumptions

Disadvantages:

- requires clear trusted starting point policy
- validator set updates are security-critical

### zk-Verified Light Client

Advantages:

- can compress verification
- attractive for bridges and constrained clients

Disadvantages:

- high implementation complexity
- proof systems add assumptions and audit burden
- not appropriate as a first mandatory design

### Committee Light Client

Advantages:

- smaller validator proof surface if committees are used
- may scale better with large validator sets

Disadvantages:

- committee selection and randomness become security-critical
- additional censorship and sampling risks

## Security Considerations

RPC deception:

- Risk: an RPC server lies about finality or state.
- Mitigation: proof-backed header, finality, and state verification.

Long-range attack:

- Risk: old validators produce a plausible alternative history.
- Mitigation: weak-subjectivity policy, trusted recent checkpoint, validator set
  update verification, and unbonding windows.

Validator set confusion:

- Risk: client verifies a finality proof against the wrong validator set.
- Mitigation: validator set commitments and update proofs.

Replay across networks:

- Risk: proof from testnet verifies on mainnet or another chain.
- Mitigation: chain ID and network ID binding.

Resource exhaustion:

- Risk: attacker sends oversized proofs to constrained clients.
- Mitigation: proof size limits and bounded verification.

Cryptographic migration:

- Risk: old proofs become unverifiable after algorithm changes.
- Mitigation: versioned crypto profiles and historical verification support.

Bridge risk:

- Risk: bridges treat weak or stale proofs as finality.
- Mitigation: bridge-specific policy must define proof freshness and failure
  handling.

## Compatibility

Changing light-client proof semantics is a major compatibility event for
wallets, bridges, exchanges, explorers, and SDKs.

Adding a proof metadata field can be compatible only if:

- it is versioned
- bounded
- ignored safely by old clients
- not required for verification before activation

Changing validator set update rules requires explicit light-client migration.

## Open Decisions

- final light-client proof schema
- final validator set update proof
- weak subjectivity period
- proof freshness policy
- supported starting points
- maximum proof size
- maximum verification time
- state proof relationship
- bridge-specific verification policy
- zk proof support, if any
- committee light-client support, if any
- light-client test vectors

## Related Specifications

- `docs/rfc/consensus/light-client-finality-proofs.md`
- `docs/rfc/networking/p2p-protocol-messages.md`
