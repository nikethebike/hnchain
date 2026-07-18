# ADR-0021: Rust Workspace Policy

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0004: Canonical Serialization
- ADR-0018: P2P Protocol Messages
- ADR-0019: Storage And State Interfaces
- ADR-0020: Implementation Language

Supersedes: None

## Context

HNChain selected Rust as the primary language for the first node prototype and
reference implementation direction.

Before source code is created, the project needs a workspace policy that defines
crate boundaries, dependency direction, safety rules, testing requirements, and
release expectations.

Without this policy, early implementation code can accidentally define protocol
behavior, couple independent subsystems, or introduce hidden dependencies that
are difficult to remove later.

## Decision

HNChain will use a single Rust Cargo workspace for the first reference node
prototype.

The workspace must be modular, specification-driven, and structured so that
protocol-critical crates are isolated from node runtime concerns.

No Rust source crate should be created until this ADR is present and referenced
from the roadmap.

## Initial Workspace Shape

The initial workspace should use the following conceptual structure:

```text
Cargo.toml
crates/
  hn-core/
  hn-crypto/
  hn-hncs/
  hn-state/
  hn-storage/
  hn-consensus/
  hn-network/
  hn-node/
  hn-rpc/
  hn-cli/
tests/
  conformance/
  integration/
```

Final crate manifests and public APIs require implementation PR review.

## Crate Responsibilities

### hn-core

Protocol-independent core types and shared domain primitives.

Examples:

- protocol version identifiers
- chain identifiers
- block height and round types
- bounded integer newtypes
- shared error categories

`hn-core` must not depend on networking, storage engines, RPC frameworks, async
runtimes, or node process code.

### hn-crypto

Cryptographic identity, public key, signature, hash, and address primitives.

This crate must wrap external cryptographic libraries behind HNChain-owned
types and interfaces.

This crate must not implement custom cryptographic algorithms.

### hn-hncs

HNChain canonical serialization implementation.

This crate must implement accepted serialization specifications and conformance
test vectors.

It must not infer protocol behavior from Rust memory layout, serde defaults, map
iteration order, or platform-specific behavior.

### hn-state

Account state, state transitions, state root interface types, and state-related
validation boundaries.

This crate must not know which storage engine persists state.

### hn-storage

Storage engine abstraction and adapter implementations.

Storage adapters must expose deterministic behavior to protocol crates and must
not leak database-specific ordering or encoding into consensus behavior.

### hn-consensus

Consensus state machine, validator set logic, vote verification interfaces,
quorum certificate handling, finality logic, fork-choice rules, and slashing
evidence validation.

This crate must not perform network I/O directly.

### hn-network

P2P transport abstraction, peer management, protocol messages, capability
negotiation, and message routing.

This crate must not define consensus validity.

### hn-node

Node composition layer.

This crate wires consensus, storage, networking, mempool, RPC, configuration,
metrics, and process lifecycle together.

The node crate may depend on implementation crates, but implementation crates
must not depend on `hn-node`.

### hn-rpc

RPC type definitions, endpoint handlers, request validation, and API versioning
surface.

RPC must call node services through explicit interfaces and must not bypass
validation layers.

### hn-cli

Command-line interface for running nodes, local development, diagnostics, and
administrative tooling.

The CLI must be treated as a client of public node interfaces, not as a private
backdoor into protocol state.

## Dependency Direction

Allowed dependency direction:

```text
hn-core
  <- hn-crypto
  <- hn-hncs
  <- hn-state
  <- hn-storage
  <- hn-consensus
  <- hn-network
  <- hn-node
  <- hn-rpc
  <- hn-cli
```

The diagram shows conceptual layering, not a requirement that each crate depends
on every previous crate.

Rules:

- lower-level crates must not depend on higher-level crates
- protocol crates must not depend on process lifecycle crates
- storage and network adapters must depend on interfaces, not on consensus
  internals
- cyclic crate dependencies are forbidden
- feature flags must not change protocol semantics

## Protocol Independence

Rust implementation details are not protocol definitions.

Protocol behavior must come from:

- accepted ADRs
- RFCs
- formal specifications
- canonical test vectors
- compatibility suites

If code and specification disagree, the discrepancy is a protocol issue, not an
opportunity to silently follow the implementation.

## Rust Version Policy

The workspace must define a Minimum Supported Rust Version.

Initial policy:

- MSRV must be explicitly declared before the first release tag
- MSRV updates require a documented compatibility note
- consensus-critical crates must avoid nightly-only features
- stable Rust is required for release builds

The exact MSRV is an open decision until the first workspace scaffold is
created.

## Unsafe Rust Policy

Unsafe Rust is forbidden by default in consensus-critical crates:

- `hn-core`
- `hn-crypto`
- `hn-hncs`
- `hn-state`
- `hn-consensus`

Any exception requires:

- written justification
- documented safety invariants
- narrow module containment
- dedicated tests
- fuzzing where applicable
- explicit review approval

Unsafe usage in non-consensus runtime crates still requires justification and
must not affect protocol validity.

## Dependency Policy

Dependencies must be minimal and justified.

Every production dependency must be reviewed for:

- license compatibility with Apache-2.0
- maintenance status
- release history
- transitive dependency footprint
- security history
- deterministic behavior
- platform support
- audit surface

Cryptographic dependencies require additional review:

- algorithm standardization
- constant-time behavior where relevant
- side-channel risk
- key and signature encoding rules
- test vector availability
- upstream audit status where available

No dependency may define HNChain protocol semantics implicitly.

## Serialization Policy

Consensus-critical data must use HNCS canonical serialization once specified.

The workspace must not use generic serialization defaults for protocol hashes,
state roots, signatures, block IDs, transaction IDs, or wire compatibility.

Human-facing JSON may exist for RPC, CLI, diagnostics, and tests, but it is not
the canonical protocol format.

## Async Runtime Policy

Async runtime selection is deferred.

The selected runtime must be isolated from protocol crates.

Consensus state transitions must remain deterministic and must not depend on
task scheduling order, wall-clock timing, cancellation timing, or runtime
implementation details.

## Error Handling Policy

The workspace must use typed errors at module boundaries.

Rules:

- protocol validation errors must be distinguishable from internal failures
- invalid external input must not panic
- panics are unacceptable for normal network, RPC, transaction, block, or state
  validation errors
- fatal process errors must be explicit at the node composition layer

Exact error crates and conventions are open decisions.

## Logging And Observability

Protocol crates must not depend on a concrete logging backend.

Node-level observability may include:

- structured logs
- metrics
- tracing spans
- health endpoints

Observability must not alter protocol behavior.

## Test Requirements

Each crate must have tests appropriate to its responsibility.

Required categories:

- unit tests for local logic
- integration tests for module boundaries
- conformance tests for canonical protocol behavior
- test vectors for serialization, hashes, signatures, addresses, blocks, and
  transactions
- fuzzing for binary parsing, network packet decoding, serialization, and
  consensus-critical validation
- regression tests for security fixes

Consensus-critical behavior must have deterministic tests.

## CI Requirements

The initial CI pipeline should include:

- formatting check
- linting
- unit tests
- integration tests
- documentation link checks where practical
- dependency audit
- license check
- unused dependency check where practical

Additional CI stages should be added before public testnet:

- fuzzing jobs
- cross-platform builds
- reproducible build checks
- benchmark smoke tests
- compatibility test vectors

## Release Build Policy

Release builds must be reproducible as a long-term objective.

Rules before production release:

- lockfile must be committed for node binaries
- build profile must be documented
- target platforms must be listed
- binary provenance must be documented
- release artifacts must be checksummed

## Feature Flags

Feature flags may control optional integrations and platform support.

Feature flags must not:

- change consensus rules
- change canonical serialization
- change address format
- change state root computation
- change transaction IDs
- enable incompatible wire formats without protocol negotiation

## Public API Rules

Public Rust APIs should be documented when exported outside a crate.

Protocol-facing APIs must describe:

- input types
- output types
- validation errors
- deterministic behavior
- versioning expectations

Internal APIs may evolve, but protocol-facing behavior must remain compatible
with accepted specifications.

## Security Considerations

Hidden coupling:

- Risk: crates become tightly coupled through convenience imports.
- Mitigation: dependency direction rules and interface boundaries.

Runtime nondeterminism:

- Risk: async scheduling, system time, randomness, or I/O timing affects
  consensus behavior.
- Mitigation: isolate runtime concerns from protocol state transitions.

Dependency supply chain:

- Risk: abandoned or compromised dependencies affect node security.
- Mitigation: dependency review, lockfile policy, audits, and CI checks.

Unsafe code:

- Risk: unsafe blocks reintroduce memory safety vulnerabilities.
- Mitigation: disallow unsafe by default in consensus-critical crates.

Specification drift:

- Risk: the Rust implementation becomes the de facto protocol definition.
- Mitigation: conformance tests and explicit specification ownership.

## Compatibility

This ADR does not define protocol data formats.

It defines implementation workspace rules for the Rust reference direction.

Independent implementations in other languages remain valid if they pass the
same protocol specifications and conformance tests.

## Open Decisions

- exact MSRV
- exact crate names
- async runtime
- error handling crate
- logging and tracing crates
- dependency review tooling
- license checking tooling
- fuzzing framework
- benchmarking framework
- CI provider and matrix
- supported target platforms
- reproducible build process

## Related Specifications

- `docs/adr/ADR-0020-implementation-language.md`
- `docs/roadmap/README.md`
