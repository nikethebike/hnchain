# ADR-0020: Implementation Language

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0018: P2P Protocol Messages
- ADR-0019: Storage And State Interfaces

Supersedes: None

## Context

HNChain needs an implementation language for the first node prototype and future
reference implementation.

The language choice affects safety, dependency policy, build reproducibility,
performance, contributor onboarding, cryptographic library availability,
tooling, long-term maintenance, and independent implementation strategy.

The initial candidate languages were Rust and Go.

## Decision

HNChain selects Rust as the primary language for the first node prototype and
reference implementation direction.

The first Rust workspace should be created only after the initial implementation
workspace policy is specified.

This decision does not require every HNChain tool, SDK, wallet, explorer, or
independent node implementation to be written in Rust.

## Rationale

Rust is a strong fit for HNChain's core node because it provides:

- memory safety without garbage collection
- explicit ownership and borrowing
- strong type system for protocol boundaries
- good performance characteristics
- mature package tooling through Cargo
- good support for no-hidden-copy binary parsing patterns
- growing ecosystem for cryptography, networking, and storage
- good fit for modular crate boundaries

For a blockchain node, memory safety and deterministic resource management are
important. Bugs in networking, serialization, storage, or consensus can become
security issues.

Rust does not prove correctness by itself. It reduces some classes of memory
errors, but protocol correctness still requires specifications, tests, review,
fuzzing, formal reasoning where needed, and audits.

## Normative Rules

### Reference Implementation Direction

The first node implementation should use Rust.

Initial conceptual workspace:

```text
crates/
  hn-core
  hn-crypto
  hn-hncs
  hn-state
  hn-storage
  hn-consensus
  hn-network
  hn-node
  hn-rpc
  hn-cli
```

Final crate names require a workspace architecture RFC before creation.

### Protocol Independence

Rust code must not define protocol behavior by accident.

Protocol behavior remains defined by accepted ADRs, RFCs, specifications, and
test vectors.

### Unsafe Rust Policy

Unsafe Rust is disallowed by default in consensus-critical crates.

Any future unsafe usage must have:

- documented justification
- safety invariants
- code review requirement
- tests
- fuzzing where applicable
- module-level containment

### Dependency Policy

Dependencies must be minimized and justified.

Consensus-critical dependencies require review for:

- maintenance status
- license compatibility
- audit surface
- transitive dependencies
- deterministic behavior
- cryptographic suitability, if applicable

### Reproducibility

The Rust workspace must support reproducible builds as a long-term goal.

Exact build reproducibility requirements are defined by release engineering
specifications.

### Independent Implementations

HNChain must remain implementable in other languages.

The Rust implementation must not rely on undocumented behavior that prevents a
Go, C++, Java, or other independent implementation from following the protocol.

## Rejected Options

### Go As Primary Node Language

Rejected for the first reference implementation direction because Go's garbage
collector and simpler type system are less aligned with HNChain's desired
low-level control over memory, binary formats, and resource-sensitive execution.

Go remains a strong candidate for SDKs, tooling, services, and independent node
implementations.

### C++ As Primary Node Language

Rejected because it increases memory safety and long-term maintenance risk.

C++ can be appropriate for specialized libraries, but should not be the first
reference node language.

### Multiple Primary Node Languages From Day One

Rejected because it would split early engineering effort before protocol test
vectors and compatibility suites are mature.

Independent implementations remain a long-term goal.

### Language Choice Before Architecture

Rejected as a general process. HNChain already completed enough foundational
architecture to choose a first implementation language, but production code
still requires accepted specifications and test vectors.

## Alternatives Considered

### Rust

Advantages:

- memory safety without garbage collection
- strong type system
- good performance
- good crate-based modularity
- strong tooling for formatting, linting, testing, and fuzzing

Disadvantages:

- steeper learning curve
- longer compile times
- async ecosystem complexity
- careful dependency governance required

### Go

Advantages:

- simple language model
- fast compilation
- strong networking ecosystem
- easy operational tooling
- broad backend developer familiarity

Disadvantages:

- garbage collection may complicate latency-sensitive paths
- weaker type-level modeling for protocol invariants
- less control over memory layout and allocation behavior

## Security Considerations

Unsafe code risk:

- Risk: unsafe Rust can reintroduce memory unsafety.
- Mitigation: unsafe disallowed by default and reviewed separately if needed.

Dependency supply-chain risk:

- Risk: transitive crates introduce vulnerabilities or abandoned code.
- Mitigation: dependency review, lockfile policy, license checks, and audits.

False confidence:

- Risk: teams treat the implementation language as proof of protocol
  correctness.
- Mitigation: specifications, conformance tests, fuzzing, and independent
  review remain mandatory.

Implementation monopoly:

- Risk: Rust reference behavior becomes protocol truth.
- Mitigation: specification-first process and compatibility test vectors.

Async complexity:

- Risk: async networking code hides liveness or cancellation bugs.
- Mitigation: explicit runtime policy, structured concurrency guidelines, and
  deterministic consensus boundaries.

## Compatibility

This ADR selects the primary implementation language. It does not change
protocol compatibility.

Public protocol APIs, wire formats, canonical serialization, and state roots
must remain language-independent.

SDKs may be implemented in Rust, Go, TypeScript, Python, Java, Swift, Kotlin,
C#, or other languages according to ecosystem needs.

## Open Decisions

- Rust minimum supported version
- workspace crate layout
- async runtime policy
- error handling policy
- dependency review policy
- fuzzing framework
- CI matrix
- release build policy
- reproducible build requirements
- no-std policy for selected crates
- FFI policy
- SDK language priority

## Related Specifications

- `docs/roadmap/README.md`
