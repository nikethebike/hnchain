# HNChain Improvement Proposals

HNIP documents define the long-term proposal process for HNChain protocol and
ecosystem changes.

HNIP stands for HN Improvement Proposal.

All significant protocol changes begin with an HNIP or with an ADR that later
points to an HNIP.

## Lifecycle

```text
Idea
  -> Draft
  -> Discussion
  -> Technical Review
  -> Security Review
  -> Implementation
  -> Testing
  -> Voting
  -> Activation
```

No stage should be skipped for consensus-critical changes.

## Categories

Initial HNIP categories:

```text
HNIP-C -> Core
HNIP-V -> VM
HNIP-N -> Networking
HNIP-S -> Standards
HNIP-I -> Informational
```

## Required Sections

Each HNIP should include:

- problem
- motivation
- specification
- rationale
- backward compatibility
- security considerations
- reference implementation, if applicable
- migration plan
- activation conditions

## Specification First

HNChain follows:

```text
HNIP -> Specification -> Discussion -> Code
```

Code is not accepted as protocol behavior before the relevant specification is
accepted.

## Compatibility Tests

HNChain should maintain a compatibility test suite.

```text
Compatibility Suite -> PASS / FAIL
```

Independent clients should be able to use the same tests to verify conformance.

HNIPs are not a replacement for protocol specifications. Accepted HNIPs point to
the RFCs, ADRs, or specifications they modify.
