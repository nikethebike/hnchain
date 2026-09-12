# ADR-0009: Consensus Architecture

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0008: Block Format

Supersedes: None

## Context

Consensus is the mechanism by which HNChain nodes agree on block history and the
resulting state root in the presence of network delay, faults, and Byzantine
behavior.

HNChain targets fast finality and high throughput, but these are engineering
targets, not accepted safety guarantees. The consensus design must prioritize
correctness, explicit assumptions, and long-term maintainability over headline
performance claims.

The protocol must avoid coupling leader selection, transaction ordering,
finality, validator accounting, network transport, and storage internals into a
single inseparable implementation.

## Decision

HNChain defines consensus as a modular subsystem composed of replaceable
protocol roles and interfaces.

Conceptual architecture:

```text
Consensus Engine
  -> Validator Set
  -> Leader Election
  -> Proposal Validation
  -> Transaction Ordering
  -> Voting
  -> Finality
  -> Evidence
  -> Checkpoints
  -> Synchronization
```

The first consensus profile should be BFT-oriented and should target safety when
fewer than one third of active voting power is Byzantine, assuming the final
algorithm satisfies its network and timing assumptions.

**Decided: initial consensus family is Tendermint-style BFT.** Asked the
user first — this is the single most consequential decision in the
project so far, since quorum certificates, finality justification,
evidence categories, and epoch transition timing all cascade from it.

Rejected the other three candidates in "Alternatives Considered" for
the *initial* profile specifically, not as a claim that they are worse
in general:

- **HotStuff-style BFT**: pipelined/linear communication is a real
  scalability advantage, but leader-failure and view-change tuning are
  subtler and the design has less production hardening at the scale of
  scrutiny this project targets for its first profile. Nothing rules
  out a HotStuff-style profile later, once the validator set is large
  enough that Tendermint's `O(n²)` vote traffic becomes a genuine
  bottleneck — "Protocol Upgrades" above already anticipates replacing
  the consensus profile.
- **DAG-based mempool plus BFT finality**: ADR-0009's own analysis
  already flags this as "significantly more complex, harder to audit"
  for a first profile — matches this project's established preference
  for well-precedented, conservatively-audited designs over novel ones
  (RFC 6962 for `hn-list-merkle-v1`, "no custom cryptography without
  external review" for signatures, ADR-0002).
- **Avalanche-style metastable consensus**: probabilistic finality
  contradicts this project's own stated deterministic-finality goal
  (Goals, above: "Deterministic finality under explicit assumptions").
- Proof of Work was already rejected (Rejected Options, above) before
  this decision.

Tendermint-style BFT: round-based, one proposer per round (Leader
Election, below, decides selection specifically), a `propose ->
prevote -> precommit -> commit` voting sequence, deterministic finality
on a `2f+1`-of-`3f+1` voting-power quorum at each voting stage (no
reorgs after commit, matching the `<1/3` Byzantine threshold this ADR's
Decision text already targeted before this choice was made) — this is
the same fault threshold, not a new one. `O(n²)` message complexity
per round is acceptable for a first profile with a moderate validator
set; revisit if validator set size later makes it a bottleneck.

## Normative Rules

### Explicit Safety Model

Every accepted consensus profile must define:

- fault threshold
- voting power model
- quorum threshold
- synchrony or partial synchrony assumptions
- finality rule
- fork-choice rule, if any
- validator set update rule
- evidence and accountability rule
- liveness assumptions

Claims about fixed finality latency are invalid unless the assumptions and
failure modes are documented.

### Modular Boundaries

Consensus modules must have explicit interfaces.

Required conceptual modules:

- validator set management
- leader or proposer selection
- proposal verification
- transaction ordering
- vote verification
- quorum certificate construction
- finality verification
- evidence verification
- checkpoint verification
- catch-up synchronization

Replacing one module must not require redefining unrelated protocol objects.

### Block Format Integration

Consensus uses the block format defined by ADR-0008.

Consensus-specific data must be committed through:

- `round`
- `epoch`
- `proposer`
- `consensus_root`
- `evidence_root`
- `justification`

Consensus must not add hidden validity rules through network packet metadata or
local node state.

### Validator Identity

Validators are identified through canonical cryptographic identity descriptors.

Validator signatures must bind to:

- protocol name
- chain ID
- network ID
- consensus profile
- height
- round
- epoch
- block hash or consensus object hash
- signing purpose

Signature verification must use the cryptographic identity specification.

### Finality

Finality must be represented by an explicit proof or certificate.

The proof must be verifiable by nodes and light clients according to the
accepted consensus profile.

Applications must not treat gossip reception, mempool inclusion, or block
proposal as finality.

### Transaction Ordering

Consensus defines the canonical order of transactions in accepted blocks.

Mempool ordering is local policy unless explicitly promoted into consensus
rules.

The final block order must produce deterministic execution and state roots.

### Leader Election

Leader or proposer selection must be specified independently from block
validation.

If randomness is used, it must come from a protocol-defined source and must be
verifiable.

Leader selection must define resistance to targeted attacks, grinding, stake
concentration, and denial-of-service amplification.

Deterministic weighted round-robin, no randomness source at all — see
ADR-0011, "Decided: deterministic weighted round-robin," which follows
directly from Tendermint-style BFT being the chosen family, the same
way the timeout/view-change mechanism below does.

### Timeout And View Change

**Decided: mechanism, not durations.** Follows directly from
Tendermint-style BFT (ADR-0009, "Decided: initial consensus family"),
not an independent choice — this is the standard Tendermint round
structure, not a novel design.

Each round has three sequential stages — `propose`, `prevote`,
`precommit` (ADR-0012, "`vote_type` registry": `prevote`/`precommit`
are the two voting stages; `propose` is the proposer's own single
signed proposal, not a vote) — each with its own timeout. Timeouts
increase with round number (monotonic backoff) rather than staying
fixed, which is what gives the protocol liveness under only *partial*
synchrony (ADR-0009's Explicit Safety Model already requires stating
synchrony assumptions): once actual network delay stays under the
now-large-enough timeout, a round eventually completes, without ever
assuming a fixed bound on message delay.

- **Propose timeout.** If a validator's propose-stage timeout expires
  without a valid proposal from the round's elected proposer
  (ADR-0011), it `prevote`s `nil` (`target_type = nil`, ADR-0012 —
  not a distinct vote type; the type is still `prevote`).
- **Prevote timeout.** If a validator's prevote-stage timeout expires
  without observing a `2f+1` `prevote` quorum for one specific block,
  it `precommit`s `nil`.
- **Precommit timeout / round advance.** If a validator's
  precommit-stage timeout expires without observing a `2f+1`
  `precommit` quorum for one specific block, the round increments —
  height stays the same, a new proposer is selected for the new round
  by the same deterministic priority algorithm (ADR-0011), and the
  three stages repeat. This *is* "view change" for this profile: there
  is no separate timeout-certificate object to construct or verify (a
  round's own absence of a qualifying `precommit` quorum before
  timeout is itself sufficient justification to advance), unlike
  designs that construct an explicit timeout certificate.

This resolves "round semantics" (Open Decisions, below) directly: a
`round` is one `propose -> prevote -> precommit` attempt at a fixed
`height`; advancing the round never changes `height`, and only a
successful `2f+1` `precommit` quorum (ADR-0012) at any round finalizes
that height and moves to the next one.

The **exact timeout durations** (base value, backoff formula) are
deliberately not decided here — tunable network-timing parameters,
the same class of decision as `MAX_TRANSACTION_SIZE` (ADR-0006) or
epoch length (ADR-0010): fixed protocol constants for a later pass,
not structural choices, and not decidable without also deciding real
network-timing assumptions this ADR's "Explicit Safety Model" requires
stating explicitly first.

### Decided: Target Block Time

```text
TARGET_BLOCK_TIME = 2 seconds
```

A distinct parameter from the round timeout durations above, not a
restatement of them: `TARGET_BLOCK_TIME` is the expected *average*
time between blocks in the happy path, needed wherever a wall-clock
duration decided elsewhere must be expressed in blocks (its first
consumer: ADR-0023's "Decided: Unbonding Period" — 21 days — converted
to `UNBONDING_PERIOD_BLOCKS` for `hn_state::apply_unstake`, since
`BlockHeader.timestamp`'s own consensus semantics are still undecided,
"Timestamp," ADR-0008, and cannot yet be relied on for a
consensus-critical maturity check). 2 seconds matches typical
Tendermint-family BFT chains at a comparable validator-set size (up to
`MAX_ACTIVE_SET_SIZE = 100`, ADR-0023) — a reasonable balance between
finality speed and network overhead.

This is a target, not an enforced minimum: this profile has no hard
minimum-block-interval rule (Timeout And View Change, above — round
timeouts only bound the *worst* case, via backoff on the unhappy path,
not the typical case), so real block production can run faster or
slower than 2 seconds depending on actual network conditions. Any
height-based conversion of a wall-clock duration using
`TARGET_BLOCK_TIME` is therefore an approximation of real elapsed
time, not a guarantee — accepted explicitly for
`UNBONDING_PERIOD_BLOCKS`'s own case, not an oversight.

### Evidence And Accountability

Consensus must define evidence formats before activating penalties such as
slashing.

Evidence must be canonical, bounded, and verifiable.

Punishment rules must be deterministic and must not depend on discretionary
operator judgment.

### Synchronization

Consensus must define how new or recovering nodes safely catch up.

Required modes:

- full history verification
- checkpoint-assisted synchronization
- snapshot-assisted synchronization, if snapshots are activated
- light-client verification

Fast synchronization must not require trusting arbitrary RPC responses.

### Protocol Upgrades

Consensus upgrades must define:

- activation condition
- activation height or epoch
- validator signaling requirements, if any
- compatibility behavior before activation
- rollback and halt behavior
- light-client impact

Changing finality rules is a major protocol change.

## Rejected Options

### Proof Of Work

Rejected for HNChain's initial direction because it conflicts with the stated
goals of fast finality and low energy consumption.

This does not mean Proof of Work is insecure. It means it does not match
HNChain's intended design constraints.

### Undocumented Hybrid Consensus

Rejected because combining ideas from multiple consensus families without a
formal safety model creates hidden failure modes.

### Fixed Small Validator Set As A Shortcut

Rejected as a default assumption because it can improve performance by reducing
decentralization.

A bounded active set may still be selected if justified by safety, performance,
and governance analysis.

### Mempool Order As Consensus

Rejected because mempool contents and arrival order differ across nodes.

### Local Time As Consensus Randomness

Rejected because local clocks are not deterministic across validators.

## Alternatives Considered

### Tendermint-Style BFT

Advantages:

- mature conceptual model
- clear safety threshold
- deterministic finality
- simpler reasoning than many highly optimized protocols

Disadvantages:

- communication overhead grows with validator set size
- very large validator sets are difficult without aggregation or committee
  mechanisms

### HotStuff-Style BFT

Advantages:

- pipeline-friendly design
- quorum certificates provide compact finality evidence
- strong fit for modular finality proofs

Disadvantages:

- implementation complexity
- leader failure and timeout tuning are subtle
- performance depends heavily on networking and signature aggregation

### DAG-Based Ordering With BFT Finality

Advantages:

- can separate data dissemination from finality
- may improve throughput under high load
- can reduce leader bottlenecks

Disadvantages:

- significantly more complex
- harder to audit
- finality and data availability interactions require careful proof

### Avalanche-Style Metastable Consensus

Advantages:

- scalable sampling-based approach
- useful design ideas for fast probabilistic agreement

Disadvantages:

- probabilistic finality semantics may not match HNChain's deterministic
  finality goal
- safety analysis differs from classical BFT assumptions

## Security Considerations

Safety violation:

- Risk: two conflicting blocks finalize at the same height.
- Mitigation: formal quorum rules, slashing evidence, signature binding, and
  consensus test vectors.

Liveness failure:

- Risk: the network stops finalizing blocks even without safety failure.
- Mitigation: explicit timeout, view-change, leader rotation, and network
  assumptions.

Long-range attacks:

- Risk: old validators create an alternative finalized history.
- Mitigation: checkpoint rules, unbonding windows, light-client security model,
  and historical validator set verification.

Targeted proposer attacks:

- Risk: attackers disrupt known future leaders.
- Mitigation: verifiable unpredictability, short lookahead, redundancy, or
  proposer rotation design.

Vote equivocation:

- Risk: validators sign conflicting consensus messages.
- Mitigation: signing context, evidence root, deterministic penalties, and
  operator key isolation.

Data unavailability:

- Risk: votes finalize a block whose body is unavailable.
- Mitigation: availability checks before voting and explicit propagation rules.

Stake centralization:

- Risk: high stake concentration becomes consensus capture.
- Mitigation: staking economics, delegation design, validator set policy, and
  governance limits.

Complexity risk:

- Risk: a novel consensus design contains subtle correctness bugs.
- Mitigation: conservative module boundaries, formal modeling, simulation,
  independent review, and staged testnets.

## Compatibility

Consensus profile changes are major protocol events unless explicitly designed
as compatible parameter updates.

Adding a new consensus message can be backward-compatible only if:

- message type is versioned
- network packet format is specified
- unsupported nodes reject it deterministically
- activation rules are defined
- finality verification remains unambiguous

Changing validator set rules, quorum thresholds, or finality semantics requires
explicit migration planning.

## Open Decisions

- `MAX_ACTIVE_SET_SIZE` value — **resolved, ADR-0023: `K = 100`**
  (derivation mechanism decided — ADR-0010, "Decided: active set
  derivation mechanism": bounded, top-K by `voting_power` descending,
  ties by ascending `validator_id`)
- voting power's maximum value bound and zero-power behavior (model,
  capping algorithm, and integer type — `u128` — all decided above;
  ADR-0010, "Decided: capped stake-weighted voting power" / "Decided:
  capping algorithm" / "Decided: voting power integer type"; cap value
  itself remains a separate open economic parameter)
- timeout and view-change *durations* (mechanism decided above —
  ADR-0009, "Timeout And View Change"; the exact base timeout and
  backoff formula are not — distinct from `TARGET_BLOCK_TIME`,
  "Decided: Target Block Time," above, which is decided)
- `EPOCH_LENGTH` value — **resolved, ADR-0023: `43_200` blocks (24
  hours at `TARGET_BLOCK_TIME`)** (transition mechanism decided,
  ADR-0010, "Epoch Boundaries")
- validator set update timing
- slashing activation model
- checkpoint interval
- light-client finality proof
- data availability rule
- consensus networking channels
- performance benchmark methodology
- formal modeling framework

## Related Specifications

- `docs/rfc/consensus/consensus-architecture.md`
- `docs/rfc/consensus/validator-set.md`
