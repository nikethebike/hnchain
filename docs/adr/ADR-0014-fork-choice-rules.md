# ADR-0014: Fork-Choice Rules

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0011: Leader Election
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0013: Finality Rules

Supersedes: None

## Context

Even in a BFT-oriented protocol with deterministic finality, nodes may observe
multiple proposed or certified blocks before finality is reached.

Fork-choice rules define how a node selects its preferred non-finalized head
while preserving the finality rule as the source of irreversible history.

Without explicit fork-choice rules, implementations may diverge during proposer
failures, network delays, round changes, or partial partitions.

## Decision

HNChain defines fork-choice as a versioned consensus rule that applies only
above the latest finalized block.

Conceptual input:

```text
ForkChoiceInput
  fork_choice_version
  consensus_profile
  finalized_anchor
  candidate_blocks
  quorum_certificates
  timeout_certificates
  validator_set_commitments
  local_safety_state
```

Conceptual output:

```text
ForkChoiceOutput
  preferred_head
  preferred_round
  locked_block
  safe_to_vote
```

This ADR does not accept a final fork-choice algorithm. Tendermint-style locks,
HotStuff-style highest QC, DAG-based ordering preference, and conservative
finalized-only operation remain candidates depending on the final consensus
profile.

## Normative Rules

### Finalized Anchor

Fork-choice must start from the latest verified finalized block.

A node must not select a preferred head that conflicts with the finalized
anchor.

### Non-Finalized Scope

Fork-choice applies only to proposed, certified, or otherwise non-finalized
blocks.

Finality rules always override fork-choice preference.

### Versioned Rule

Every fork-choice profile is versioned.

Nodes must not infer fork-choice behavior from client implementation, local
database ordering, network arrival order, or RPC responses.

### Deterministic Inputs

Fork-choice decisions that affect consensus votes must be derived from
canonical consensus objects:

- block headers
- parent links
- quorum certificates
- timeout certificates, if used
- validator set commitments
- finality proofs
- local safety state defined by the consensus profile

Local gossip arrival order may influence when a node learns about a block, but
must not be the rule for comparing two known candidates.

### Safety State

If the consensus profile uses locks, preferred rounds, or other local safety
state, that state must be explicitly specified.

Local safety state must never permit voting for a block that violates the
profile's safety rule.

### Vote Eligibility

Fork-choice may determine whether it is safe to vote for a proposal.

The vote decision must bind to:

- finalized anchor
- candidate block
- height
- round
- epoch
- validator set
- highest known certificate or lock, if applicable

### Tie-Breaking

Tie-breaking must be deterministic and specified.

Invalid tie-breakers include:

- network arrival order
- peer ID
- local database key
- wall-clock timestamp
- RPC response order

### Reorganization

Reorganization above the finalized anchor may be allowed by a profile.

Reorganization of finalized blocks is invalid unless the chain is operating
outside its stated safety assumptions and recovery procedures are invoked.

### Evidence Interaction

Fork-choice must not ignore valid evidence that invalidates a candidate block,
vote, certificate, or validator action.

The final evidence rules are defined separately.

## Rejected Options

### Longest Chain Rule

Rejected as the default for HNChain's BFT-oriented direction because chain
length alone does not express validator quorum, rounds, locks, or finality.

### First Seen Block Wins

Rejected because network arrival order differs across nodes and is easily
manipulated.

### Highest Transaction Count Wins

Rejected because transaction count is not a safety signal and can be
manipulated.

### Fork-Choice Can Override Finality

Rejected because it destroys finality semantics.

### Hidden Client-Specific Preference

Rejected because independent implementations must make compatible consensus
decisions.

## Alternatives Considered

### Finalized-Only Operation

Advantages:

- simplest application semantics
- avoids exposing non-finalized head complexity to clients
- conservative for early implementation

Disadvantages:

- less useful for mempool and block propagation optimization
- may reduce responsiveness before finality

### Tendermint-Style Lock Rules

Advantages:

- established BFT safety model
- explicit prevote/precommit behavior
- clear locked block semantics

Disadvantages:

- timeout tuning is subtle
- lock/unlock rules must be specified carefully
- communication overhead increases with validator set size

### HotStuff-Style Highest QC

Advantages:

- aligns with quorum-certificate chaining
- pipeline-friendly
- compact safety signal

Disadvantages:

- safety relies on correct QC chain interpretation
- view-change and timeout interactions are subtle

### DAG Preference Rule

Advantages:

- can support higher throughput when ordering and data dissemination are
  separated
- may reduce leader bottlenecks

Disadvantages:

- significantly more complex
- harder to expose simple client semantics
- data availability and ordering proofs become critical

## Security Considerations

Safety violation:

- Risk: fork-choice causes validators to vote for conflicting candidates.
- Mitigation: explicit safety state, locks or highest-QC rules, and test
  vectors.

Network-order manipulation:

- Risk: attackers influence preferred heads by controlling message timing.
- Mitigation: deterministic comparison rules independent of arrival order.

Finality confusion:

- Risk: applications treat preferred head as finalized.
- Mitigation: explicit client states and finality proof requirement.

Liveness failure:

- Risk: overly conservative fork-choice prevents progress.
- Mitigation: timeout and unlock rules defined by the consensus profile.

Equivocation hiding:

- Risk: fork-choice ignores conflicting proposals or votes.
- Mitigation: evidence pool integration and bounded evidence checks.

Resource exhaustion:

- Risk: attackers send many competing candidates.
- Mitigation: bounded candidate storage, certificate verification limits, and
  peer scoring in networking policy.

## Compatibility

Changing fork-choice rules is a major consensus change if it can affect voting,
proposal acceptance, or finality.

Adding local fork-choice hints can be compatible only if they do not change
consensus validity and are explicitly non-normative.

Light clients must not rely on fork-choice preference unless a proof-backed
pre-finality mode is specified.

## Open Decisions

- initial fork-choice profile
- lock semantics
- unlock semantics
- highest-QC selection rule
- timeout certificate role
- nil vote interaction
- tie-breaking rule
- candidate retention window
- reorganization limits above finalized anchor
- evidence interaction
- client exposure of non-finalized heads
- light-client pre-finality support
- fork-choice test vector suite

## Related Specifications

- `docs/rfc/consensus/fork-choice-rules.md`
