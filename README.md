# HNChain

HNChain is a specification-first Layer 1 blockchain architecture focused on
security, deterministic execution, modularity, predictable economics, long-term
maintainability, and open protocol evolution.

The project is currently in the architecture and specification phase.

## Current Status

Status: Draft documentation and architectural specifications.

Protocol implementation has not started.

The repository contains an initial Rust workspace scaffold for the future
reference implementation.

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
- [Validator Set Model](docs/adr/ADR-0010-validator-set-model.md)
- [Leader Election](docs/adr/ADR-0011-leader-election.md)
- [Vote Messages And Quorum Certificates](docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md)
- [Finality Rules](docs/adr/ADR-0013-finality-rules.md)
- [Fork-Choice Rules](docs/adr/ADR-0014-fork-choice-rules.md)
- [Slashing And Accountability](docs/adr/ADR-0015-slashing-and-accountability.md)
- [Synchronization Checkpoints](docs/adr/ADR-0016-synchronization-checkpoints.md)
- [Light-Client Finality Proofs](docs/adr/ADR-0017-light-client-finality-proofs.md)
- [P2P Protocol Messages](docs/adr/ADR-0018-p2p-protocol-messages.md)
- [Storage And State Interfaces](docs/adr/ADR-0019-storage-state-interfaces.md)
- [Implementation Language](docs/adr/ADR-0020-implementation-language.md)
- [Rust Workspace Policy](docs/adr/ADR-0021-rust-workspace-policy.md)
- [Core Primitive Types RFC](docs/rfc/core/primitive-types.md)

## Specification First

HNChain follows:

```text
architecture -> specifications -> review -> implementation -> testing -> audit
```

Code must not define protocol behavior by accident. Protocol behavior is defined
by accepted specifications.

## Development

The Rust workspace is intentionally minimal at this stage.

Required toolchain:

- Rust `1.97.1`
- `rustfmt`
- `clippy`

Expected checks:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

## Licensing

Code is licensed under Apache-2.0.

Documentation is licensed under CC BY 4.0 unless a file states otherwise.
