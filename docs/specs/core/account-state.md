# HNChain Core Specification: Account State

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## 1. Scope

This document specifies the conceptual account state model for HNChain.

It defines account boundaries, versioning principles, deterministic transition
requirements, extension rules, and module interaction boundaries.

This document does not specify binary encoding, storage column layout,
cryptographic algorithms, transaction format, fee model, VM execution, or
consensus rules. Those must be defined in separate specifications.

This specification is constrained by
`docs/adr/ADR-0000-protocol-invariants.md`.

## 2. Design Goals

- Account state is a first-class protocol object.
- All account changes are deterministic.
- Account structure is extensible without implicit behavior.
- Serialization and hashing are canonical.
- Unknown fields and extensions have defined compatibility behavior.
- Storage, execution, RPC, wallet, and explorer modules depend on interfaces,
  not concrete account implementations.

## 3. Account Object

An account is identified by an address and represented by a versioned account
state object.

Conceptual structure:

```text
AccountState
  envelope_version
  account_type
  address
  identity
  balances
  nonce
  permissions
  metadata
  assets
  extensions
  lifecycle
```

Each section has its own version and validation rules.

The canonical binary representation must be defined in the serialization
specification before implementation.

## 3.1 Account Types

Initial conceptual account types:

```text
standard
validator
smart_contract
system
```

Account type is consensus-relevant.

Unknown account types are invalid unless activated by protocol upgrade rules.

Type-specific behavior must be defined by the owning protocol specification.

## 3.2 Identity Binding

An account does not depend on one raw public key.

Accounts bind to cryptographic identity through key descriptors, role bindings,
or identity commitments defined by the cryptographic identity specification.

This allows key rotation, multisignature, session keys, hardware wallets, and
future post-quantum migration without redefining account identity.

## 4. Required Sections

### 4.1 Envelope

The envelope identifies the account state format.

Required fields:

- `envelope_version`
- `account_type`
- `address`
- `section_versions`

The envelope is mandatory and must be included in state hashing.

### 4.2 Versioned Storage Record

Every persisted account-related record must have an explicit storage envelope.

Conceptual structure:

```text
StorageRecord
  record_version
  owner
  record_type
  payload
```

Fields:

- `record_version`: version of the storage record envelope.
- `owner`: account address or protocol namespace that owns the record.
- `record_type`: canonical identifier for the contained payload type.
- `payload`: canonical bytes defined by the payload specification.

Storage records describe persistence boundaries. They do not define account
semantics.

### 4.3 Balance State

Balance state represents native HNChain currency and protocol-approved asset
balances.

Balance mutation is allowed only through valid state transitions.

Balance values must use fixed-width unsigned integers. Floating point values are
forbidden in consensus state.

### 4.4 Nonce State

Nonce state prevents transaction replay and defines account transaction ordering.

The nonce model must be specified before transaction validation is implemented.
At minimum, the model must define:

- nonce width
- initial nonce
- increment rules
- replay protection domain
- behavior for failed execution

### 4.5 Permission State

Permission state defines account-level authorization capabilities.

Permission changes are consensus-relevant and must be included in state hashing.

Permissions must not depend on wall-clock time unless consensus time semantics
are formally defined.

Conceptual permission capabilities may include:

- owner control
- administration
- operation
- viewing or read-only access
- voting delegation
- spending limits
- session authorization
- emergency lock or recovery

These concepts are not activated by this specification. Each permission feature
requires explicit state transition rules before implementation.

### 4.6 Metadata State

Metadata state contains bounded protocol-level account metadata.

Metadata is not a general-purpose unbounded key-value store. Size limits,
allowed value types, canonical encoding, and fee implications must be specified
before activation.

### 4.7 Asset State

Asset state describes assets supported by the account.

The asset model must distinguish between:

- native currency
- protocol-level assets
- contract-defined assets
- bridged assets

No asset class may be introduced without explicit supply, ownership, and
validation rules.

### 4.8 Extension State

Extension state allows protocol evolution.

Every extension has:

- extension identifier
- extension version
- criticality flag
- canonical payload
- activation rules

Unknown critical extensions make the account state invalid for nodes that do not
support them.

Unknown non-critical extensions may be preserved but must not affect consensus
behavior unless supported and activated by protocol rules.

Extension loading is lazy by default.

The core account record contains an extension registry, not necessarily all
extension payloads. Extension payloads are loaded only when required by
validation, execution, indexing, or proof generation.

Lazy loading must not change consensus behavior. A transition that requires an
extension must declare that dependency through canonical transition input or
protocol-defined access rules.

### 4.9 Lifecycle State

Lifecycle state is part of consensus account state.

Conceptual lifecycle:

```text
Created -> Active -> Frozen -> Deprecated -> Archived -> Destroyed
```

Lifecycle meanings:

- `Created`: account exists but may not yet be fully active.
- `Active`: account may participate according to its permissions and balances.
- `Frozen`: account exists but selected transitions are blocked by protocol
  rules.
- `Deprecated`: account is valid but should no longer receive new capabilities
  except migration or archival operations.
- `Archived`: account is no longer on the hot execution path but remains
  provable according to archival rules.
- `Destroyed`: account has completed a protocol-defined removal process.

Lifecycle transitions must define:

- required authorization
- allowed source states
- allowed target states
- balance and asset handling
- metadata and extension handling
- storage record handling
- state root impact
- proof and archival behavior

`Destroyed` must not mean silent deletion unless state tree and historical proof
rules explicitly allow it.

## 5. Deterministic State Transitions

All account changes occur through deterministic transition functions:

```text
previous_account_state + valid_transition_input -> next_account_state
```

A transition is valid only if:

- input encoding is canonical
- preconditions are satisfied
- authorization checks pass
- all numeric operations are overflow-safe
- resulting account state passes validation
- resulting state root is deterministic

Nondeterministic inputs are forbidden in consensus transitions, including:

- local system time
- random number generators without consensus-defined randomness
- network response ordering
- floating point arithmetic
- map iteration order without canonical ordering

## 6. Module Boundaries

```text
Transaction Pool
      |
      v
Transaction Validation
      |
      v
State Transition Engine -----> Cryptography
      |
      v
State Database -----------> Storage Engine
      |
      v
State Root / Proof System
      |
      v
Consensus
```

Boundary rules:

- Consensus verifies state roots and transition validity, but must not own
  account storage internals.
- Storage stores canonical bytes and indexed views, but must not define account
  semantics.
- RPC exposes documented account views, but must not bypass state validation.
- Wallets construct transactions, but must not be trusted for state correctness.
- HNVM may request account reads and writes through an execution interface, but
  must not mutate storage directly.

## 7. State Bloat Architecture

HNChain account state must be designed so storage growth can be bounded,
priced, pruned, or archived by future protocol rules.

Required architectural support:

- versioned storage records
- bounded metadata
- lazy extension loading
- explicit account lifecycle
- extension payload size limits
- storage accounting hooks
- compatibility with future rent policy

A rent policy is not activated by this specification. However, the account model
must preserve enough structure to introduce rent, deposits, storage fees,
pruning, or archival incentives without changing the meaning of existing account
fields.

## 8. Compatibility Rules

Account evolution follows SemVer at the specification level.

Patch changes:

- clarify validation rules without changing accepted canonical state
- add non-consensus documentation

Minor changes:

- add optional non-critical sections
- add extension records with deterministic defaults
- add RPC fields that do not change consensus behavior

Major changes:

- change canonical encoding
- change hashing rules
- change required fields
- change transition semantics
- change meaning of an existing field

Major changes require explicit migration and governance activation rules.

## 9. Security Risks

State bloat:

- Risk: metadata and extensions can grow without bound.
- Mitigation: size limits, fees, pruning rules, and storage accounting.

Replay attacks:

- Risk: nonce ambiguity allows transaction replay.
- Mitigation: formally specified nonce domain and signing payload.

Extension confusion:

- Risk: nodes interpret unknown extensions differently.
- Mitigation: criticality flags and activation rules.

Serialization ambiguity:

- Risk: multiple encodings represent the same state.
- Mitigation: canonical binary encoding and rejection of non-canonical forms.

Unauthorized mutation:

- Risk: permissions are bypassed by VM, RPC, or internal modules.
- Mitigation: one state transition engine owns all consensus mutations.

Parallel execution conflicts:

- Risk: concurrent account writes produce nondeterministic results.
- Mitigation: declared access sets, conflict detection, or deterministic
  scheduler rules.

Lazy extension inconsistency:

- Risk: different node classes load different extension payloads and validate
  different effective account state.
- Mitigation: extension dependencies must be declared by protocol rules and
  included in validation, execution, and proof requirements.

## 10. Open Architecture Decisions

The following decisions are required before implementation:

- cryptographic identity
- account address derivation
- native token unit and numeric width
- nonce model
- canonical serialization format
- state trie or alternative authenticated data structure
- storage backend abstraction
- metadata size limits
- extension registry and activation process
- account lifecycle rules
- transaction access-list model
