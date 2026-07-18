# HNChain Consensus RFC: Fork-Choice Rules

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0010-validator-set-model.md`
- `docs/adr/ADR-0011-leader-election.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0013-finality-rules.md`
- `docs/adr/ADR-0014-fork-choice-rules.md`
- `docs/adr/ADR-0015-slashing-and-accountability.md`

## 1. Purpose

This RFC defines the conceptual fork-choice model for HNChain consensus.

It specifies how nodes reason about proposed and certified blocks before
finality, without weakening the finality rules.

## 2. Scope

This RFC defines:

- finalized anchor rule
- non-finalized candidate scope
- fork-choice inputs and outputs
- safety state requirements
- deterministic tie-breaking requirements
- reorganization boundaries
- client exposure requirements

This RFC does not define:

- final consensus algorithm
- final lock or unlock rule
- final timeout values
- final evidence penalties
- final networking peer scoring

## 3. Fork-Choice Input

Conceptual structure:

```text
ForkChoiceInputV1
  fork_choice_version
  consensus_profile
  finalized_anchor
  candidate_blocks
  quorum_certificates
  timeout_certificates
  validator_set_commitments
  local_safety_state
```

Inputs must be canonical consensus objects or explicitly specified local safety
state.

## 4. Fork-Choice Output

Conceptual structure:

```text
ForkChoiceOutputV1
  preferred_head
  preferred_round
  locked_block
  safe_to_vote
```

`preferred_head` is the node's best non-finalized candidate.

`locked_block` is present only for profiles with lock semantics.

`safe_to_vote` indicates whether the profile allows voting for the candidate.

## 5. Finalized Anchor

Fork-choice starts at the latest finalized block.

Candidates that do not descend from the finalized anchor are invalid for
preferred-head selection.

## 6. Candidate Validation

A candidate block must pass cheap structural checks before entering
fork-choice:

- supported block version
- correct chain and network
- valid parent relationship
- valid proposer eligibility
- valid basic consensus metadata
- bounded size
- valid quorum certificate, if required by profile

Full execution may be required before voting depending on the consensus profile.

## 7. Safety State

Consensus profiles may define local safety state.

Examples:

```text
locked_block
locked_round
highest_qc
highest_timeout_certificate
last_vote
```

The profile must define persistence, update, and recovery rules for safety
state.

## 8. Preference Rules

Final preference rules are open.

Candidate comparison dimensions may include:

- higher certified round
- higher justified height
- stronger quorum certificate
- valid descendant of locked block
- deterministic tie-breaker

The final profile must define exact ordering.

## 9. Tie-Breaking

Tie-breaking must be deterministic.

Allowed candidate dimensions may include canonical hashes or profile-defined
numeric fields.

Rejected tie-breakers:

- network arrival order
- peer identity
- local storage order
- local wall-clock time
- transaction count

## 10. Reorganization Boundary

Reorganization is allowed only above the finalized anchor and only according to
the consensus profile.

Finalized history is not reorganized by fork-choice.

If conflicting finalized blocks are detected, the node must enter a safety
incident path defined by incident response and consensus recovery procedures.

## 11. Client Exposure

RPC and SDK layers must distinguish:

```text
preferred_head
certified_head
finalized_head
```

Clients must not treat `preferred_head` or `certified_head` as finalized.

## 12. Security Requirements

Implementations must reject:

- candidates conflicting with the finalized anchor
- candidates with invalid parent links
- candidates with invalid proposer eligibility
- candidates with malformed certificates
- certificates for another chain or network
- unsafe votes under profile rules
- tie-breaks based on local arrival order
- unbounded candidate sets

Implementations must bound:

- candidate cache size
- certificate verification time
- fork-choice recomputation time
- safety state persistence size
- non-finalized branch depth

## 13. Test Vectors

The accepted version must include test vectors for:

- single candidate selection
- conflicting candidates above finalized anchor
- candidate conflicting with finalized anchor rejection
- higher-QC candidate selection, if applicable
- lock rule enforcement, if applicable
- unlock rule enforcement, if applicable
- deterministic tie-break
- reorganization above finalized anchor
- finalized block reorganization rejection
- unsafe vote rejection

Test vectors are mandatory before production implementation.

## 14. Open Decisions

- final fork-choice profile
- final lock semantics
- final highest-QC rule
- final timeout certificate rule
- final tie-breaker
- final candidate cache limits
- final branch depth limits
- final client exposure policy
- final incident path for conflicting finality
