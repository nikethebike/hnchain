# HNChain Roadmap

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## Purpose

HNChain roadmap documentation defines release phases, readiness criteria,
release strategy, upgrade policy, deprecation policy, and long-term support.

The roadmap is not a calendar promise.

It describes how the protocol evolves and what criteria must be satisfied before
moving between phases.

## Development Phases

```text
Research
  -> Prototype
  -> Developer Preview
  -> Devnet
  -> Testnet
  -> Mainnet
  -> Long-Term Support
```

Each phase requires explicit exit criteria.

## Implementation Language

The proposed primary language for the first node prototype and reference
implementation direction is Rust.

Reference:

- `docs/adr/ADR-0020-implementation-language.md`

## Release Flow

```text
Specification
  -> Implementation
  -> Testing
  -> Audit
  -> Release Candidate
  -> Production
```

## Versioning

HNChain uses SemVer-style versioning:

```text
Major.Minor.Patch
```

- Major: incompatible protocol changes
- Minor: backward-compatible functionality
- Patch: compatible fixes

Exact protocol versioning rules must be defined in release specifications.
