# HNChain Core Specification: Cryptographic Identity

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## 1. Scope

This document specifies the conceptual cryptographic identity model for
HNChain.

It defines key descriptors, signature envelopes, key roles, verification
contexts, algorithm lifecycle, and module boundaries.

This document does not define address derivation, canonical binary encoding,
hash algorithms, transaction format, validator set mechanics, or wallet file
formats.

This specification is constrained by:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`

## 2. Design Goals

- Support multiple signature algorithms without changing account semantics.
- Avoid inferring cryptographic meaning from byte length or address shape.
- Separate account keys, validator keys, network keys, governance keys, and
  bridge keys.
- Bind every signature to a protocol-defined context.
- Preserve room for post-quantum migration.
- Prevent consensus behavior from depending on wallet or RPC conventions.

## 3. Cryptographic Identity

A cryptographic identity is a protocol object that authorizes actions through
one or more keys.

Conceptual structure:

```text
CryptographicIdentity
  identity_version
  identity_id
  key_descriptors
  role_bindings
  rotation_policy
  recovery_policy
  lifecycle
```

`identity_id` is not necessarily an address. Address derivation is defined by
the address format specification.

## 4. Key Descriptor

Conceptual structure:

```text
KeyDescriptor
  descriptor_version
  algorithm_id
  key_id
  key_role
  public_key
  lifecycle
  validity_rules
  metadata_commitment
```

Required rules:

- `algorithm_id` is mandatory.
- `key_id` must be derived or assigned by protocol rules.
- `key_role` must be explicit.
- `public_key` must use the encoding defined by the algorithm specification.
- `metadata_commitment` must not contain unbounded metadata.

## 5. Signature Envelope

Conceptual structure:

```text
SignatureEnvelope
  envelope_version
  algorithm_id
  key_reference
  verification_context
  signature
```

Required rules:

- `algorithm_id` must match the referenced key descriptor.
- `verification_context` is mandatory.
- `signature` must pass algorithm-specific canonicality checks.
- Unknown algorithms are rejected unless explicitly activated by protocol rules.

## 6. Verification Context

Verification context prevents replay across domains.

Conceptual fields:

```text
VerificationContext
  protocol_name
  network_id
  chain_id
  object_type
  object_version
  signature_purpose
```

Examples of `signature_purpose`:

- transaction authorization
- validator vote
- validator proposal
- peer handshake
- governance vote
- bridge attestation
- account recovery

Exact field encoding is defined by canonical serialization.

## 7. Key Roles

Initial key roles:

```text
account_signing
validator_consensus
validator_network
governance
bridge_operator
identity_recovery
```

Role rules:

- A key may be used only for roles assigned by the account or validator state.
- Role changes are consensus-relevant.
- Role changes must be auditable.
- Validator network keys must not authorize fund movement unless explicitly
  bound to an account-signing role.

## 8. Algorithm Registry

HNChain maintains a protocol-level algorithm registry.

Conceptual structure:

```text
AlgorithmRegistryEntry
  registry_version
  algorithm_id
  algorithm_name
  lifecycle
  supported_roles
  public_key_format
  signature_format
  verification_rules
  test_vector_commitment
```

The registry is normative. Implementations must not activate algorithms by local
configuration alone.

## 9. Algorithm Lifecycle

Algorithm lifecycle:

```text
Proposed -> Active -> Deprecated -> Disabled
```

Rules:

- `Proposed` algorithms are not valid for consensus signatures.
- `Active` algorithms are valid only for listed roles.
- `Deprecated` algorithms may verify existing signatures according to migration
  rules.
- `Disabled` algorithms are rejected except for explicitly specified historical
  verification.

## 10. Module Boundaries

```text
Wallet / CLI
    |
    v
Signing Interface
    |
    v
Signature Envelope
    |
    v
Validation Layer -----> Algorithm Registry
    |
    v
State Transition Engine
```

Boundary rules:

- Wallets create signatures but do not define consensus validity.
- RPC transports signature envelopes but does not reinterpret them.
- P2P authenticates peers using network identity rules, not account-signing
  rules.
- Consensus verifies validator roles, not only cryptographic signatures.
- Storage persists canonical key descriptors but does not define verification
  semantics.

## 11. Security Requirements

- Private keys are never consensus data.
- Consensus code must reject non-canonical public keys and signatures.
- Signing payloads must be canonical and context-bound.
- Cryptographic implementations must be constant-time where secret data is
  processed.
- Custom cryptographic primitives are forbidden unless accepted by a dedicated
  ADR and external review.
- Test vectors are required for every active algorithm.
- Batch verification must be optional and must produce the same accept or reject
  result as individual verification.

## 12. Open Architecture Decisions

- primary account signing suite
- primary validator consensus suite
- genesis algorithm registry
- multi-key account semantics
- key rotation transaction format
- recovery policy model
- threshold and multisignature model
- hardware wallet requirements
- post-quantum migration plan
- cryptographic library selection
