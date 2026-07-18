# ADR-0001: Extended Account-Based State Model

Status: Accepted

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants

## Context

HNChain is a Layer 1 blockchain designed around specification-first architecture.
The network requires a state model suitable for balances, smart contracts,
permissions, account metadata, multiple assets, and protocol extensions.

The state model must support deterministic state transitions, long-term protocol
compatibility, modular implementation, and safe evolution of account fields.

## Decision

HNChain uses an extended account-based state model.

Each account is an independent state object. Account state changes are performed
only through deterministic state transition functions defined by protocol
specifications.

An account contains a versioned core envelope and independently versioned
sections:

- identity
- balance state
- nonce state
- permission state
- metadata state
- asset state
- extension state
- lifecycle state

New account fields must be added through versioned sections or extension
records. Existing fields must not change their canonical meaning without a new
version.

Account state storage must support versioned records and lazy extension loading.
The account core must remain small enough for frequent validation paths, while
large or optional extensions are loaded through an explicit extension registry.

## Rationale

An account-based model is the most direct fit for HNChain's planned smart
contract, permission, identity, and multi-asset capabilities.

Compared with a UTXO model, it simplifies account identity, contract interaction,
nonce handling, wallet UX, and permission-aware state transitions.

The extended model avoids treating accounts as simple balance containers. This
is necessary because HNChain accounts are protocol-level entities, not only
ledger addresses.

## Consequences

Benefits:

- Natural model for smart contracts and account-level permissions.
- Explicit account lifecycle and metadata handling.
- Easier integration with wallets, identity, RPC, explorer, and governance.
- Supports protocol evolution through versioned sections.
- Allows storage layout and execution logic to evolve independently.

Costs:

- Global state contention can limit parallel execution if access sets are not
  specified carefully.
- Account mutation rules must be strict to prevent nondeterminism.
- State growth must be controlled through rent, pruning, archival policy, or
  another formally specified mechanism.
- Extension fields increase compatibility complexity.
- Lazy extension loading requires strict rules to avoid inconsistent validation
  between full nodes, validators, archival nodes, RPC nodes, and light clients.

## Alternatives Considered

### UTXO Model

Advantages:

- Strong local reasoning about spendability.
- Natural parallel validation for independent UTXOs.
- Clear audit trail for coin movement.

Disadvantages:

- More complex smart contract model.
- Harder account permissions and identity integration.
- Multi-step contract interactions require additional abstraction.

Rejected because HNChain targets account-native smart contracts, identity,
permissions, and protocol-managed account metadata.

### Minimal Account Model

Advantages:

- Simpler initial implementation.
- Smaller state object surface.

Disadvantages:

- Forces later protocol-breaking changes for permissions, metadata, assets, and
  extensions.
- Encourages hidden application-level conventions.

Rejected because it conflicts with long-term compatibility and explicit
protocol design.

## Security Considerations

- Account state must have canonical serialization.
- Unknown critical extensions must not be silently ignored.
- State transitions must be deterministic across all supported nodes.
- Metadata must have size limits and validation rules.
- Permission changes must be auditable and protected against replay.
- Nonce rules must prevent replay and transaction ordering ambiguity.
- Extension activation must be governed by protocol versioning rules.

## Compatibility

Account data is versioned at the envelope and section levels.

Backward-compatible changes may add optional non-critical sections or extension
records with deterministic defaults.

Backward-incompatible changes require a protocol upgrade and explicit migration
rules.

The account lifecycle is part of consensus state. HNChain recognizes the
following conceptual lifecycle states:

```text
Created -> Active -> Frozen -> Deprecated -> Archived -> Destroyed
```

Exact transition permissions, state root implications, archival rules, and
destroy semantics must be defined before implementation.

## State Bloat Controls

HNChain account architecture must support the following mechanisms:

- versioned storage records
- explicit account lifecycle state
- bounded metadata
- lazy extension loading
- future rent policy or another economic storage-control policy
- pruning and archival rules

The rent policy does not have to be active in the initial protocol version, but
the storage and account model must not prevent its later introduction.

## Related Specifications

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/specs/core/account-state.md`
