# HNChain Documentation

HNChain documentation is organized as a specification-first system.

The whitepaper explains direction and motivation. ADRs record architectural
decisions. RFCs and specifications define implementable protocol behavior. HNIPs
define long-term change proposals. Developer docs explain how to build on top of
accepted protocol interfaces.

## Documentation Map

```text
docs/
  whitepaper/
    HNChain-Whitepaper-v0.1-draft.md

  adr/
    ADR-0000-protocol-invariants.md
    ADR-0001-account-state-model.md
    ADR-0002-cryptographic-identity.md
    ADR-0003-address-format.md
    ADR-0004-canonical-serialization.md
    ADR-0005-hash-algorithms.md
    ADR-0006-transaction-format.md
    ADR-0007-state-tree.md
    ADR-0008-block-format.md
    ADR-0009-consensus-architecture.md

  specs/
    core/
      account-state.md
      cryptographic-identity.md
      address-format.md
      canonical-serialization.md
      hash-algorithms.md
      genesis.md
      transaction-format.md
      state-tree.md
      block-format.md

  rfc/
    consensus/
      consensus-architecture.md
    networking/
    storage/
    rpc/
    wallet/
    explorer/
    sdk/
    cli/
    api/
    interoperability/

  hnvm/
    README.md

  hnip/
    README.md

  governance/
    constitution.md

  standards/
    README.md

  performance/
    README.md

  roadmap/
    README.md

  ecosystem/
    README.md

  security/
    README.md

  developer/
    README.md
```

## Document Families

### Whitepaper

Purpose:

- mission
- philosophy
- economic direction
- consensus direction
- major trade-offs
- roadmap

The whitepaper is not sufficient for implementation.

### ADR

Purpose:

- record architectural decisions
- explain rationale and alternatives
- capture consequences and risks
- define dependency order between decisions

ADR documents are decision records, not full protocol specifications.

### Specifications

Purpose:

- define exact protocol objects
- define deterministic behavior
- define canonical formats
- define compatibility rules
- define security requirements

Specifications are normative once accepted.

### RFC

Purpose:

- define subsystem-level implementable behavior
- define network packets, RPC methods, storage layouts, and protocol APIs
- provide test vectors and interoperability requirements

RFCs should reference ADRs and specifications instead of redefining them.

### HNVM

Purpose:

- define virtual machine semantics
- define bytecode and execution model
- define resource metering
- define contract ABI
- define host functions and safety rules

### HNIP

Purpose:

- propose protocol and ecosystem changes
- document motivation, specification, compatibility, and activation rules
- coordinate long-term governance

### Governance

Purpose:

- define protocol evolution principles
- define constitutional constraints
- define governance process boundaries
- document compatibility and activation expectations

### Security

Purpose:

- define threat models
- define security review requirements
- define audit and bug bounty expectations
- define incident response
- document residual risks

### Standards

Purpose:

- define ecosystem interfaces
- define asset standards
- define metadata and event expectations
- define conformance tests for interoperable applications

Standards describe behavior and compatibility. They do not define consensus
truth unless explicitly accepted by protocol specifications.

### Performance

Purpose:

- define benchmark methodology
- define standard performance profiles
- define reporting requirements
- define regression tracking
- document measured trade-offs

Performance documents must report conditions, not only headline numbers.

### Roadmap

Purpose:

- define development phases
- define release readiness criteria
- define upgrade and deprecation policy
- define long-term support expectations
- document success metrics

Roadmap documents define strategy and criteria, not calendar promises.

### Ecosystem

Purpose:

- document official tools
- document ecosystem services
- define compatibility expectations
- define voluntary certification concepts
- keep protocol and tooling boundaries clear

Ecosystem tools must not define hidden protocol behavior.

### Developer Docs

Purpose:

- explain how to build applications
- explain SDK usage
- explain wallet integration
- explain RPC usage
- provide tutorials after protocols are specified

Developer docs must not define consensus truth.

## Status Rules

Document status values:

- `Draft`: early document, not accepted
- `Proposed`: reviewed direction, still open for approval
- `Accepted`: normative decision or specification
- `Deprecated`: retained for history, no longer recommended
- `Superseded`: replaced by a newer document

Production implementation must not rely on Draft or Proposed documents as final
protocol truth.
