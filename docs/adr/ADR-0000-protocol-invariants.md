# ADR-0000: Protocol Invariants

Status: Accepted

Date: 2026-07-18

Version: 0.1.0

## Context

HNChain is designed as a long-lived Layer 1 blockchain. Its protocol must remain
deterministic, auditable, and compatible across independent implementations.

Before defining cryptography, addresses, serialization, transactions, state, or
blocks, the project requires a set of protocol invariants. These invariants are
binding rules for all future architecture decisions and implementations.

## Decision

HNChain defines protocol invariants as the highest-level architectural contract.

All ADRs, specifications, implementations, tests, and protocol upgrades must
preserve these invariants unless a future major protocol revision explicitly
changes them through governance and migration rules.

## Invariants

### Deterministic Execution

Given the same valid protocol input and the same previous state, every compliant
node must compute the same output state.

```text
previous_state + canonical_input -> next_state
```

Any implementation detail that can produce divergent results across platforms is
forbidden in consensus logic.

### Deterministic State Root

Every compliant node must compute the same state root for the same canonical
state.

State root computation must depend only on canonical protocol data.

### Canonical Protocol Representation

Every consensus object must have one canonical binary representation.

Non-canonical encodings of consensus objects must be rejected before they reach
state transition logic.

### Hashes Over Canonical Bytes

Protocol hashes are computed only over canonical binary representations.

No hash may be computed over JSON, map iteration order, display strings, local
memory layout, or implementation-specific encodings.

### Validation Before Mutation

All consensus state changes must pass through the validation layer before state
mutation.

HNVM, RPC, CLI, wallet, P2P, and internal services must not bypass state
transition validation.

### No Hidden Consensus Dependencies

Consensus behavior must not depend on:

- local wall-clock time, except through explicitly defined consensus-time rules
- local timezone
- local filesystem behavior
- CPU architecture
- operating system behavior
- network message arrival order
- random number generators without consensus-defined randomness
- floating point arithmetic
- unordered map iteration
- external APIs

### Explicit Versioning

Every protocol object, network packet, storage record, and public API must be
versioned.

Unknown versions must have defined accept, reject, preserve, or upgrade
behavior.

### Backward Compatibility By Design

Backward-compatible changes must be explicitly distinguished from
backward-incompatible changes.

Backward-incompatible consensus changes require a protocol upgrade process and
defined migration rules.

### Modular Ownership

Each subsystem owns its domain responsibilities:

- cryptography defines keys, signatures, verification, and algorithm agility
- address format defines account identifiers and address encoding
- serialization defines canonical binary representation
- hashing defines digest algorithms and domain separation
- transaction format defines signed user intent and validation inputs
- state tree defines authenticated state commitment
- block format defines ordered consensus records

No subsystem may silently redefine another subsystem's semantics.

### Security By Default

Protocol designs must assume adversarial inputs, malformed peers, replay
attempts, resource exhaustion, and implementation diversity.

Security-sensitive behavior must be specified before implementation.

## Required ADR Dependency Order

The initial ADR dependency order is:

```text
ADR-0000 Protocol Invariants
  -> ADR-0001 Account Model
  -> ADR-0002 Cryptographic Identity
  -> ADR-0003 Address Format
  -> ADR-0004 Canonical Serialization
  -> ADR-0005 Hash Algorithms
  -> ADR-0006 Transaction Format
  -> ADR-0007 State Tree
  -> ADR-0008 Block Format
```

Dependency rationale:

```text
Keys -> Addresses -> Serialization -> Hashes -> Transactions -> State -> Blocks
```

Cryptographic identity precedes address format because public key and signature
algorithm choices affect key length, signature length, algorithm identifiers,
rotation rules, and address derivation.

Address format precedes serialization because account identifiers appear in
transactions, state objects, receipts, RPC responses, and storage keys.

Canonical serialization precedes hashing because protocol hashes must be defined
over canonical bytes.

Hash algorithms precede transaction, state tree, and block formats because those
objects commit to serialized data through hashes.

## Security Considerations

Violating these invariants can cause consensus splits, replay vulnerabilities,
state corruption, or incompatible independent implementations.

Any proposal that introduces nondeterminism or unspecified encoding is rejected
until the behavior is made explicit.

## Compatibility

This ADR is normative for all future specifications.

Changing an invariant is a major protocol change.
