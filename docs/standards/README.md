# HNChain Standards

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## Purpose

HNChain Standards, abbreviated as HNS, define shared ecosystem interfaces and
behavioral expectations.

Standards describe behavior. They do not require one specific implementation.

## Initial Standard Families

```text
HNS-1 -> Fungible Tokens
HNS-2 -> Non-Fungible Tokens
HNS-3 -> Multi Asset
HNS-4 -> Soulbound
HNS-5 -> Governance
HNS-6 -> Stable Assets
HNS-7 -> Wrapped Assets
```

## Required Sections

Each HNS should include:

- problem
- motivation
- interface
- behavior
- metadata format
- event model
- extension points
- compatibility
- security considerations
- reference implementation, if applicable
- test vectors or conformance tests

## Compatibility Boundary

HNS documents are ecosystem standards unless explicitly accepted as
protocol-level requirements.

Wallets, explorers, SDKs, and applications may rely on HNS interfaces for
interoperability, but consensus validity is defined by accepted protocol
specifications.
