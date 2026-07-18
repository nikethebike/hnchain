# HNChain

HNChain is a specification-first Layer 1 blockchain architecture focused on
security, deterministic execution, modularity, predictable economics, long-term
maintainability, and open protocol evolution.

The project is currently in the architecture and specification phase.

## Current Status

Status: Draft documentation and architectural specifications.

Production implementation has not started.

The current whitepaper candidate is:

- [HNChain Whitepaper v0.1 Draft](docs/whitepaper/HNChain-Whitepaper-v0.1-draft.md)

## Documentation

Start here:

- [Documentation Map](docs/README.md)

Core documents:

- [Protocol Invariants](docs/adr/ADR-0000-protocol-invariants.md)
- [Account State Model](docs/adr/ADR-0001-account-state-model.md)
- [Cryptographic Identity](docs/adr/ADR-0002-cryptographic-identity.md)
- [Address Format](docs/adr/ADR-0003-address-format.md)
- [Canonical Serialization](docs/adr/ADR-0004-canonical-serialization.md)
- [Hash Algorithms](docs/adr/ADR-0005-hash-algorithms.md)
- [Transaction Format](docs/adr/ADR-0006-transaction-format.md)
- [State Tree](docs/adr/ADR-0007-state-tree.md)
- [Block Format](docs/adr/ADR-0008-block-format.md)
- [Consensus Architecture](docs/adr/ADR-0009-consensus-architecture.md)

## Specification First

HNChain follows:

```text
architecture -> specifications -> review -> implementation -> testing -> audit
```

Code must not define protocol behavior by accident. Protocol behavior is defined
by accepted specifications.

## Licensing

Code is licensed under Apache-2.0.

Documentation is licensed under CC BY 4.0 unless a file states otherwise.
