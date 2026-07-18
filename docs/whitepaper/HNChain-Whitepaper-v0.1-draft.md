# HNChain Whitepaper

Version: 0.1.0-draft

Author: Newscape Inc.

Project: HNChain

License: CC BY 4.0

Date: 2026-07-18

Status: Draft Complete Candidate

Document Type: Open Specification

## Abstract

HNChain is a specification-first Layer 1 blockchain architecture focused on
security, deterministic execution, modularity, predictable economics, long-term
maintainability, and open protocol evolution.

This whitepaper defines the strategic architecture and design philosophy for
HNChain. It describes the major subsystems, their responsibilities, the risks
they introduce, and the documents required before production implementation.

The whitepaper is not a full protocol specification. Consensus rules, binary
formats, network packets, RPC methods, state transitions, VM semantics, and
storage layouts must be defined in ADRs, RFCs, and technical specifications.

## Status And Scope

This document is a `0.1.0-draft` whitepaper candidate.

It is suitable for architectural review, public discussion, and planning.

It is not sufficient for mainnet implementation.

Normative protocol behavior is defined only by accepted ADRs, RFCs, and
specifications.

Document status summary:

- Accepted: protocol invariants and extended account model.
- Proposed: cryptographic identity, address format, canonical serialization,
  hash profiles, transaction format, state tree, block format, and consensus
  architecture.
- Draft: economics, HNVM, P2P, storage, governance, security, standards,
  interoperability, performance, ecosystem, and roadmap.

## Table Of Contents

- Chapter I: Mission
- Chapter II: Why HNChain Exists
- Chapter III: Core Principles
- Chapter IV: Engineering Targets
- Chapter V: High-Level Architecture
- Chapter VI: Repository Direction
- Chapter VII: Account Model
- Chapter VIII: Cryptographic Identity
- Chapter IX: Address Model
- Chapter X: Canonical Serialization
- Chapter XI: Hashing
- Chapter XII: Genesis Block And Manifest
- Chapter XIII: Economic Model
- Chapter XIV: Consensus
- Chapter XV: HNVM
- Chapter XVI: P2P Network
- Chapter XVII: Storage Engine
- Chapter XVIII: Cryptography
- Chapter XIX: Accounts And Wallet Architecture
- Chapter XX: RPC, API And SDK
- Chapter XXI: Explorer
- Chapter XXII: Wallet
- Chapter XXIII: Governance And Protocol Evolution
- Chapter XXIV: Security Model And Threat Analysis
- Chapter XXV: Token Standards And Digital Assets
- Chapter XXVI: Interoperability And Cross-Chain Architecture
- Chapter XXVII: Performance And Scalability
- Chapter XXVIII: HN Ecosystem Architecture
- Chapter XXIX: Roadmap, Release Strategy And Long-Term Vision
- Documentation System
- Glossary
- References
- Non-Goals
- Current Status

## Chapter I: Mission

HNChain is a high-performance Layer 1 blockchain protocol designed for
security, high throughput, predictable economics, and long-term maintainability.

The central design theme of HNChain is protocol durability:

```text
A blockchain designed to evolve for decades without losing its foundations.
```

HNChain is designed to be:

- secure by design
- scalable without abandoning decentralization
- practical for developers
- economically predictable
- maintainable for decades
- independent from any single company
- fully open source

HNChain follows a specification-first process:

```text
architecture -> specifications -> review -> implementation -> testing -> audit
```

Code must not define protocol behavior by accident. Protocol behavior is defined
by accepted specifications.

## Chapter II: Why HNChain Exists

Modern blockchains have demonstrated that decentralized systems can secure
digital value, coordinate open applications, and operate without a single
central owner. They have also shown that every architecture makes trade-offs.

Bitcoin demonstrates the value of a simple monetary model, conservative protocol
change, and a narrow security-focused design. Its trade-off is limited
throughput and limited native programmability.

Ethereum demonstrates the value of general-purpose smart contracts and a large
developer ecosystem. Its trade-offs include architectural complexity and fee
volatility during periods of high demand.

Solana demonstrates that high throughput and low fees are possible under an
aggressive performance-oriented architecture. Its trade-offs include higher
validator hardware expectations and a more complex runtime and networking
design.

HNChain is not designed as a direct replacement for these networks.

HNChain aims to combine conservative security principles with modern performance
engineering, modular protocol design, and a development process built around
formal specifications.

The project should introduce new engineering choices only where they are
justified by clear benefits and documented trade-offs.

## Chapter III: Core Principles

Security is more important than speed.

Speed is more important than marketing claims.

Simplicity is more important than accidental complexity.

The protocol must not depend on one company, one implementation, one client, one
wallet, one explorer, or one infrastructure provider.

All consensus behavior must be deterministic, versioned, documented, and
testable.

Every component should be understandable in isolation.

If a module can be replaced without rewriting the whole system, the architecture
is moving in the right direction.

## Chapter IV: Engineering Targets

The following numbers are engineering targets, not protocol guarantees.

They must be validated through formal design, implementation benchmarks,
network simulation, adversarial testing, and long-running public testnets before
being treated as production claims.

| Parameter | Target |
| --- | ---: |
| Throughput | 100,000+ TPS |
| Finalization | < 2 seconds |
| Transaction confirmation | 1-2 seconds |
| Block time | 400-800 ms |
| Median user fee | < 0.001 USD equivalent |
| Network uptime target | 99.99% |

These goals are intentionally ambitious. Reaching them requires trade-offs in
consensus design, validator requirements, state access, storage, networking, VM
execution, fee policy, and decentralization assumptions.

No implementation may market these values as achieved until independent
benchmarking confirms them under documented conditions.

## Chapter V: High-Level Architecture

HNChain is composed of independent subsystems with explicit boundaries.

```text
Wallet
  |
  v
JSON-RPC / gRPC / REST API
  |
  +-------------------+
  |                   |
  v                   v
Transaction Pool    Smart Contracts / HNVM
  |                   |
  +---------+---------+
            |
            v
Consensus Engine
  |
  v
Block Production
  |
  v
State Database
  |
  v
Storage Engine
  |
  v
P2P Network
```

Boundary rules:

- RPC exposes protocol APIs but does not define consensus validity.
- The transaction pool orders pending transactions but does not mutate consensus
  state.
- HNVM executes deterministic contract logic through a state access interface.
- Consensus verifies blocks, votes, and state transitions but does not own
  storage internals.
- Storage persists canonical records but does not define protocol semantics.
- P2P transports messages but does not decide validity.

## Chapter VI: Repository Direction

The initial codebase should be organized around subsystem boundaries.

Conceptual layout:

```text
hnchain/
  core/
  consensus/
  network/
  storage/
  crypto/
  rpc/
  wallet/
  explorer/
  sdk/
  cli/
  tests/
```

This layout is not yet an implementation decision. Final module names depend on
the selected implementation language and workspace tooling.

The architecture must preserve replaceability:

- consensus can evolve without rewriting wallet code
- storage backend can change without rewriting state transition semantics
- cryptographic algorithms can be added without redefining addresses
- RPC transports can evolve without changing consensus objects
- HNVM can evolve without bypassing validation

## Chapter VII: Account Model

HNChain uses an extended account-based state model.

Each account is an independent state object containing balances, nonce,
permissions, metadata, supported assets, lifecycle state, and extensions.

All account changes occur through deterministic state transitions.

Account evolution is versioned. New fields are introduced through section
versions or extension records, not by silently changing existing semantics.

Reference:

- `docs/adr/ADR-0001-account-state-model.md`
- `docs/specs/core/account-state.md`

## Chapter VIII: Cryptographic Identity

HNChain uses algorithm-agile cryptographic identity.

Keys and signatures are not raw byte blobs. They are versioned descriptors with
explicit algorithms, roles, lifecycle, and verification context.

The protocol must support long-term migration from classical cryptography to
future post-quantum or hybrid schemes without redefining account semantics.

Reference:

- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/specs/core/cryptographic-identity.md`

## Chapter IX: Address Model

HNChain addresses are versioned protocol identifiers.

Consensus uses canonical binary address payloads. Human-readable address strings
are external representations for wallets, CLI, RPC, and explorers.

The address format must support account, contract, validator, protocol, bridge,
and identity namespaces.

Example user-facing shape:

```text
hn1qzv7...
hn1ab89...
hn1x92...
```

These examples are not final addresses. The exact human-readable prefix,
checksum, payload length, and derivation rules require ADR acceptance and test
vectors.

Reference:

- `docs/adr/ADR-0003-address-format.md`
- `docs/specs/core/address-format.md`

## Chapter X: Canonical Serialization

HNChain uses HNCS: HNChain Canonical Serialization.

Every consensus object has one canonical binary representation.

Hashes, signatures, state roots, transaction identifiers, and block identifiers
are computed only over canonical bytes.

Reference:

- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/specs/core/canonical-serialization.md`

## Chapter XI: Hashing

HNChain uses versioned hash profiles with mandatory domain separation.

The protocol must never call a bare hash function for consensus data.

Hash profiles define algorithm identifiers, digest lengths, allowed uses,
lifecycle, and test vectors.

Reference:

- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/specs/core/hash-algorithms.md`

## Chapter XII: Genesis Block And Manifest

### 12.1 Purpose

The genesis block is the first immutable point in HNChain history.

It should not be treated as a place for slogans or advertising.

It should provide:

- a deterministic starting point
- historical context
- chain identity
- initial state commitment
- documentation commitments
- a reproducible genesis manifest

### 12.2 Genesis Header

Conceptual genesis header:

```text
Genesis Header
  -> Version
  -> Timestamp
  -> Chain ID
  -> Genesis Hash
  -> Initial State Root
  -> Genesis Manifest Hash
  -> Genesis Message
```

The exact block header format is defined by ADR-0008 Block Format.

The exact canonical encoding is defined by HNCS.

### 12.3 Genesis Message

HNChain should include a bounded UTF-8 genesis message.

Recommended maximum size:

```text
512 bytes
```

The message is included in the genesis commitment and therefore becomes
immutable.

Candidate messages:

```text
HNChain Genesis - An open protocol built for long-term trust, transparency and interoperability.
```

```text
Protocol over platform. Specification before implementation.
```

The final message must be selected before genesis generation.

### 12.4 Genesis Manifest

HNChain should define a genesis manifest.

Conceptual structure:

```text
Genesis Manifest
  Chain Name: HNChain
  Protocol Version: ...
  Genesis Time: ...
  Chain ID: ...
  Whitepaper Hash: ...
  Specification Hash: ...
  License: Apache-2.0
  Genesis Message: ...
```

The hash of the manifest is included in the genesis block.

This allows anyone to verify that the published documents correspond to the
version used at network launch.

### 12.5 Document Commitments

Genesis may commit to:

- whitepaper hash
- protocol specification hash
- ADR set hash
- HN Constitution hash
- compatibility test suite hash
- first release tag hash

Document hashes are useful only if the hashing process is reproducible.

The process must define included files, ordering, archive format, line-ending
policy, and hash profile.

### 12.6 Security Requirements

Genesis data must be:

- canonical
- bounded
- reproducible
- tied to a chain ID
- tied to an initial state root
- tied to a documented genesis manifest

Nodes must reject genesis data that does not match the configured chain ID and
genesis hash.

Reference:

- `docs/specs/core/genesis.md`

## Chapter XIII: Economic Model

### 12.1 Why HNC Exists

The native coin of a Layer 1 blockchain must have a protocol role.

HNChain does not treat HNC as a token that exists only because a blockchain is
expected to have one. HNC must serve concrete network functions.

HNC is used for:

- transaction fees
- staking
- validator participation
- smart contract execution fees
- DAO and protocol operations
- network security incentives

Governance participation may involve HNC, but HNChain should not assume that one
coin equals one unit of political control. Governance requires separate design
because direct plutocratic voting can concentrate protocol power.

If HNC does not support at least one necessary protocol function, its role must
be reconsidered.

### 12.2 Monetary Policy

The proposed maximum supply target is:

```text
100,000,000 HNC
```

The initial economic direction is:

```text
Max Supply = Fixed Forever
Additional Emission = 0%
```

This creates a simple and predictable monetary policy.

The number `100,000,000 HNC` is an economic design parameter, not a magic
constant. It must be validated through distribution modeling, validator reward
simulation, fee-market analysis, and ecosystem requirements.

This is not yet an accepted monetary policy. A complete tokenomics specification
must define:

- genesis allocation
- validator rewards
- fee distribution
- burn policy
- treasury policy
- staking requirements
- slashing economics
- bridge reserve handling
- governance control limits
- long-term security budget after fee-only operation

Security warning:

A fixed supply with no emission can create long-term validator incentive risk if
fees are too low to fund network security. The fee model must be economically
simulated before acceptance.

### 12.3 Fee Burning

A candidate fee distribution model is:

```text
70% -> validators
30% -> burned
```

This model is not accepted yet.

It requires economic modeling because aggressive fee burning and very low user
fees may conflict with validator revenue, spam resistance, and network security.

The fee specification must define:

- base fee or congestion pricing
- priority fees, if any
- minimum fee floor
- storage fees or rent hooks
- contract execution metering
- validator distribution rules
- burn accounting
- fee refunds
- failed transaction fee behavior
- anti-spam economics

### 12.4 Staking

The initial staking direction is open participation.

Any user should be able to stake HNC according to protocol-defined rules and
receive a share of network fees or other approved rewards.

If consensus uses stake weight, larger stake may increase participation
probability or reward share. This must be balanced against centralization risk.

The staking specification must define:

- self-staking
- delegated staking, if supported
- validator bonding
- unbonding period
- reward accounting
- slashing conditions
- stake warm-up and cool-down
- stake concentration limits, if any
- validator performance measurement
- withdrawal rules

The minimum stake is intentionally not defined in this whitepaper draft.

Minimum stake is one of the most sensitive economic parameters because it affects
validator accessibility, Sybil resistance, decentralization, and operational
security.

### 12.5 Validators

HNChain should not begin with a fixed assumption that the validator set must be
small.

The design goal is to support large validator participation, potentially:

```text
1,000+
2,000+
5,000+
10,000+
```

These are scale targets, not accepted protocol parameters.

The active validator set must be determined by the consensus algorithm, staking
rules, network performance requirements, safety assumptions, and decentralization
targets.

Architectural warning:

Very large validator sets make fast finality harder. Voting complexity,
signature aggregation, network latency, leader rotation, and data availability
must be designed together. A network cannot safely promise sub-second blocks and
unbounded validator participation without a rigorous consensus model.

### 12.6 Development Funding

Many blockchain projects fund development through direct allocation to a
foundation or ongoing emission.

HNChain should avoid hidden or discretionary funding mechanisms.

The preferred direction is transparent development funding that is separately
specified, publicly visible, and approved through governance.

Candidate mechanisms include:

- genesis development allocation with vesting
- protocol treasury funded by a defined share of fees
- community-approved grants
- ecosystem fund with public reporting
- no protocol treasury, relying only on external funding

None of these mechanisms is accepted yet.

Each option has trade-offs. A treasury can fund maintenance and audits, but it
also creates governance capture risk. No treasury reduces protocol-level control,
but may leave critical infrastructure underfunded.

### 12.7 Governance

HNChain governance must separate economic ownership from protocol control.

The naive model:

```text
1 HNC = 1 vote
```

is rejected as the default governance principle because it can turn wealth
concentration into direct protocol control.

Governance design requires a separate specification.

Models to evaluate:

- delegated voting
- reputation-weighted participation
- quadratic voting
- validator and staker chambers
- time-locked voting power
- technical council with limited scope
- off-chain signaling plus on-chain activation

Security warning:

Governance is part of the attack surface. Poor governance can become a protocol
exploit even when cryptography and consensus are correct.

### 12.8 Inflation and Halving

The initial economic direction is:

```text
Annual Inflation = 0%
Halving = Not Planned
```

HNChain does not currently adopt a Bitcoin-style halving schedule.

Halving is closely tied to Bitcoin's Proof-of-Work issuance model. HNChain must
define consensus and validator incentives before deciding whether any periodic
reward reduction mechanism is needed.

With fixed supply and no continuing emission, the network must eventually rely
on fees, treasury policy, or another explicitly defined mechanism to fund
security.

### 12.9 Consensus As Economic Core

The economic model cannot be finalized before consensus.

Most blockchain systems derive from or combine known consensus families:

- Proof of Work
- Proof of Stake
- Delegated Proof of Stake
- Proof of History-style ordering
- Avalanche-style metastability
- HotStuff-style BFT
- Tendermint-style BFT
- DAG-based systems such as Narwhal and Bullshark

HNChain may define a new consensus algorithm or a carefully justified
combination of existing ideas.

This must not be done by branding alone. A consensus proposal must include:

- safety proof or formal argument
- liveness assumptions
- validator set model
- network timing model
- adversarial model
- economic incentive model
- slashing and accountability model
- finality rule
- performance analysis
- implementation complexity analysis

If HNChain creates a strong consensus architecture, that will be more important
than creating another VM, another SDK, or another token distribution model.

## Chapter XIV: Consensus

### 13.1 What Consensus Means

Consensus is the mechanism by which independent nodes agree on the same
blockchain state, even when some participants are faulty, offline, delayed, or
malicious.

If consensus is designed or implemented incorrectly:

- double spending may become possible
- the network may split into incompatible histories
- malicious validators may rewrite or stall history
- applications may observe inconsistent finality
- users may lose trust in the chain

Consensus is therefore not merely an algorithm. It is the security foundation of
the network.

### 13.2 Requirements Before Algorithm Selection

HNChain must define consensus requirements before accepting a concrete
implementation.

The initial requirements are:

- fast finality under stated security assumptions
- no Proof of Work mining
- no normal-mode forks after finalization
- Byzantine fault tolerance
- modular consensus boundaries
- deterministic block validation
- explicit validator accountability
- documented liveness assumptions
- documented network timing assumptions
- safe new-node synchronization

### 13.3 Finality Target

HNChain targets finalization within:

```text
<= 2 seconds
```

This is an engineering target, not a guarantee.

A finalized block should be practically irreversible as long as the protocol's
security assumptions hold.

The consensus specification must define what finality means exactly:

- vote threshold
- validator set used for the decision
- height and round semantics
- equivocation handling
- conditions for rollback, if any
- checkpoint interaction
- light-client verification

### 13.4 Normal-Mode Fork Avoidance

HNChain should aim for one confirmed block at each height in normal operation.

This simplifies application development, indexing, wallet UX, bridges, and
contract assumptions.

However, the protocol must still define behavior under partitions, delayed
messages, byzantine leaders, validator churn, and equivocation. A claim of
"no forks" is not acceptable unless the fault model explains when that property
holds and when liveness may pause instead.

### 13.5 No Proof Of Work

HNChain does not use Proof of Work mining.

Reasons:

- high energy expenditure
- probabilistic finality
- slower confirmation under conservative security assumptions
- security budget tied to external mining economics

Removing Proof of Work does not remove security costs. It moves security into
validator incentives, key management, staking, slashing, governance, networking,
and consensus correctness.

### 13.6 Byzantine Fault Tolerance

The initial safety target is:

```text
up to 1/3 byzantine validators
```

This is a common target for many BFT-style protocols.

The final consensus specification must define whether the threshold is measured
by validator count, stake weight, committee weight, or another protocol-defined
weighting system.

The specification must also define behavior when the assumption is violated.

### 13.7 Horizontal Scaling

HNChain should support scaling without forcing centralization through excessive
hardware, bandwidth, or latency requirements.

The protocol must not promise linear TPS growth as validator count increases.
Throughput depends on transaction execution, state access conflicts, networking,
data availability, signature verification, mempool behavior, and storage.

Scaling approaches to evaluate:

- parallel transaction execution
- declared access sets
- deterministic scheduling
- signature aggregation
- pipelined block production
- data availability optimizations
- state snapshots
- validator committees, if compatible with security requirements

### 13.8 Consensus Pipeline

HNChain should separate the block confirmation process into testable stages:

```text
Transaction
  -> Signature Verification
  -> Mempool
  -> Leader Selection
  -> Block Creation
  -> Block Validation
  -> Voting
  -> Finalization
  -> Propagation
```

Each stage must have a specification, validation rules, metrics, and failure
behavior.

### 13.9 Leader Selection

Predictable leaders can become targets for denial-of-service, bribery,
censorship, or network-level attacks.

HNChain should investigate leader selection that is difficult to predict too far
in advance.

Candidate inputs:

- verifiable randomness
- stake or validator weight
- epoch state
- prior finalized randomness
- anti-grinding rules

Security warning:

Leader selection randomness is consensus-critical. It must not depend on local
randomness, wall-clock time, or manipulable leader-provided data without
anti-grinding analysis.

### 13.10 Voting

The initial direction is a multi-stage voting protocol, likely two-stage or
phase-based.

Conceptual flow:

```text
Proposal
  -> Validation
  -> Prevote or Prepare
  -> Precommit or Commit
  -> Finalization
```

Exact names are not accepted yet.

The voting specification must define:

- vote message format
- signed vote payload
- quorum threshold
- timeout behavior
- round changes
- equivocation evidence
- aggregation rules
- duplicate vote handling
- validator set changes
- light-client proof format

### 13.11 Finalization

A block becomes final only after receiving enough valid consensus votes under
the active validator set and active protocol rules.

After finalization:

```text
Block N -> Immutable under protocol assumptions
```

Changing a finalized block should require violating the stated safety
assumptions, such as exceeding the tolerated byzantine threshold or breaking
cryptographic assumptions.

The protocol must define how finalized checkpoints are stored, verified,
pruned, and served to new nodes.

### 13.12 Validator Responsibilities

Validators perform several protocol duties:

- receive transactions
- verify signatures
- validate transaction preconditions
- participate in mempool propagation
- produce blocks when selected
- validate proposed blocks
- vote on valid blocks
- propagate consensus messages
- maintain local state
- serve synchronization data according to node role

Validator behavior must be observable and auditable.

### 13.13 Penalties And Accountability

Validators may be penalized for protocol violations such as:

- signing conflicting blocks
- signing conflicting votes
- persistent unavailability
- invalid block proposals
- censorship behavior, if measurable by protocol rules
- data withholding, if applicable

Possible sanctions include:

- reduced rewards
- temporary removal from active set
- stake slashing, if Proof-of-Stake-style security is accepted
- forced cooldown
- governance-visible evidence records

Slashing must not be introduced casually. Incorrect slashing rules can destroy
honest validator funds during network partitions, client bugs, or ambiguous
protocol behavior.

### 13.14 New Node Synchronization

New-node synchronization is a first-class consensus concern.

A new node should be able to:

```text
Install HNNode
  -> Connect to peers
  -> Verify history or trusted checkpoints
  -> Reconstruct or download state
  -> Verify state root
  -> Enter observing mode
  -> Become eligible to validate, if authorized
```

HNChain should support multiple sync modes:

- full historical verification
- checkpoint-assisted verification
- snapshot-based state sync
- archival node operation
- light-client verification

Security warning:

Fast synchronization must not become blind trust. Snapshots require state root
verification, checkpoint rules, proof formats, and peer diversity.

### 13.15 Modular Consensus Architecture

HNChain should investigate modular consensus boundaries:

```text
Leader Election
  -> Transaction Ordering
  -> Consensus Voting
  -> Finality
```

Potential benefits:

- leader election can evolve without rewriting block validation
- transaction ordering can evolve without redefining vote semantics
- finality proofs can be reused by light clients and bridges
- testing can isolate safety-critical components

Costs:

- more interfaces to specify
- higher risk of mismatched assumptions between modules
- more complex upgrade and compatibility rules
- harder end-to-end reasoning if boundaries are weak

Architectural rule:

Consensus modularity is allowed only if module interfaces preserve the same
safety and liveness assumptions end to end.

### 13.16 HN Consensus Research Direction

The current research direction remains:

```text
HN Consensus
  -> Leader Selection
  -> Fast Voting
  -> Finality Proof
  -> Checkpoints
```

This suggests a BFT-style or hybrid-finality design.

HNChain should not claim a novel consensus protocol until it has:

- safety proof or formal argument
- liveness argument
- adversarial model
- network model
- validator set model
- performance model
- economic model
- implementation plan
- test strategy

## Chapter XV: HNVM

### 14.1 What A Virtual Machine Is

A virtual machine is the deterministic execution environment for smart
contracts.

HNVM must guarantee:

- the same result on every compliant node
- deterministic execution
- execution isolation
- bounded resource usage
- safe access to blockchain state
- predictable failure behavior
- performance behavior that can be measured and optimized

If two validators receive the same valid input and the same previous state, HNVM
must produce the same output state, emitted events, receipts, and resource
accounting.

### 14.2 Why Not EVM As The Primary VM

Ethereum Virtual Machine is an important engineering achievement and has a large
ecosystem.

HNChain should still avoid making EVM compatibility the primary execution model.

Reasons:

- stack-oriented execution complicates some optimization strategies
- Solidity and EVM tooling have known classes of developer errors
- gas schedules and opcode semantics carry historical constraints
- some operations are expensive because of legacy design choices
- EVM compatibility can force protocol decisions that conflict with HNChain's
  long-term modularity goals

This does not mean EVM is poor engineering. It means EVM was designed for a
different historical context and should not automatically define HNChain's
execution model.

EVM compatibility may be reconsidered later as a bridge, compatibility layer, or
secondary execution environment, but not before HNVM is specified.

### 14.3 Why Not Copy Solana Runtime

Solana demonstrates that high-performance contract execution is possible.

HNChain should study useful ideas such as explicit account access, parallel
execution, and runtime-level scheduling.

HNChain should not copy the Solana programming model wholesale because:

- the developer experience can be difficult for newcomers
- memory and account handling require high expertise
- the ecosystem is strongly oriented around Rust
- runtime complexity can raise audit and maintenance cost

The goal is to learn from existing systems without inheriting their full
complexity.

### 14.4 HNVM Goals

HNVM should be:

- deterministic
- safe by default
- practical to audit
- efficient to execute
- independent from a single source language
- compatible with long-term versioning
- explicit about state access

HNVM must be specified before production smart contracts are supported.

### 14.5 Execution Pipeline

Contracts are not executed directly from source code.

The conceptual pipeline is:

```text
Contract Source
  -> Compiler
  -> HN Bytecode
  -> HNVM
  -> Deterministic State Changes
```

Only validated bytecode executes in consensus.

Compiler behavior affects developer tooling, but consensus validity depends on
bytecode, VM version, ABI, resource metering, and state transition rules.

### 14.6 Bytecode And ISA

HNVM should use a small, stable, well-documented instruction set architecture.

Candidate conceptual instruction families:

```text
LOAD
STORE
CALL
RETURN
VERIFY
TRANSFER
HASH
SIGN_VERIFY
JUMP
COMPARE
```

These are not final opcodes.

The goal is not the smallest possible instruction count. The goal is an ISA that
is understandable, stable, efficient, testable, and suitable for formal review.

A smaller ISA can reduce audit surface, but if it is too small it may move
complexity into compilers or host functions. HNVM must balance VM simplicity
against compiler and runtime complexity.

### 14.7 Determinism

HNVM forbids nondeterministic consensus behavior.

Forbidden inside consensus execution:

- non-protocol randomness
- direct Internet access
- local filesystem access
- operating system time
- thread scheduling that affects results
- floating point nondeterminism
- host-specific behavior
- reflection over local runtime state

Randomness, if supported, must come from a protocol-defined source with clear
security assumptions.

### 14.8 Memory Model

HNVM should separate memory into explicit domains:

```text
ReadOnly
  -> block and transaction context

Temporary
  -> execution-local variables and frames

Persistent
  -> blockchain state accessed through validated interfaces
```

Memory rules must define:

- bounds checking
- allocation limits
- call frame limits
- persistent storage access
- read/write permissions
- rollback on failure
- serialization of stored values

Contracts must not mutate persistent state except through the state access
interface.

### 14.9 Resource Metering

HNVM needs gas or another deterministic resource metering system.

The initial direction is a resource-category model:

```text
CPU steps + Memory usage + Storage reads/writes = Fee input
```

This must not be measured by real wall-clock time, host CPU counters, operating
system scheduling, or machine-specific performance.

Resource accounting must be deterministic and protocol-defined.

The metering specification must define:

- instruction step costs
- memory allocation costs
- persistent storage read costs
- persistent storage write costs
- event and receipt costs
- contract call costs
- hash and signature verification costs
- failure and rollback costs
- refunds, if any
- maximum execution limits

Security warning:

Overly simple metering can underprice expensive operations. Overly detailed
metering can become difficult to maintain and hard for developers to reason
about. HNVM must balance predictability, DoS resistance, and simplicity.

### 14.10 Parallel Execution And Access Graphs

Many smart contract calls do not touch the same state.

HNVM should investigate parallel execution through explicit access declaration.

Conceptual model:

```text
HN Access Graph
  reads:
    Account A
    Token B
    NFT C
  writes:
    Wallet D
```

If two transactions have disjoint write sets and compatible read sets, they may
be candidates for parallel execution.

The access graph must be part of deterministic validation. It cannot be an
untrusted performance hint unless the protocol defines fallback and conflict
behavior.

The parallel execution specification must define:

- declared read sets
- declared write sets
- conflict detection
- read-after-write behavior
- write-after-write behavior
- deterministic scheduling
- rollback semantics
- access-list validation
- consequences for undeclared access
- interaction with fees

Security warning:

Incorrect access declarations can cause nondeterministic execution, invalid
state roots, or exploitable concurrency bugs. HN Access Graph must be treated as
a consensus feature, not a local optimization.

### 14.11 Versioning

Every deployed contract must declare:

```text
VM Version
Compiler Version
Language Version
ABI Version
```

The VM version defines execution semantics.

The compiler version is metadata unless the protocol chooses to restrict accepted
compiler outputs.

The language version helps reproducibility and developer tooling, but consensus
validity is based on bytecode and VM rules.

### 14.12 VM Upgrades

HNVM upgrades must preserve old contract behavior.

The network should support multiple VM versions during long transition periods:

```text
HNVM v1
HNVM v2
HNVM v3
```

HNVM v2 must not silently change the meaning of HNVM v1 bytecode.

Upgrade rules must define:

- activation height or epoch
- allowed deployment versions
- execution behavior for old contracts
- migration tools
- deprecation policy
- disabled features
- test vectors for each active VM version

### 14.13 Static Analysis And Safety Checks

Before contract deployment, HNVM tooling should perform automated checks.

Candidate checks:

- memory bounds violations
- invalid bytecode
- unreachable code
- stack or frame limit violations
- integer overflow risks
- forbidden host calls
- undeclared state access
- impossible resource bounds
- obvious infinite loops where detectable

These checks do not replace audits.

They reduce common errors and make unsafe patterns visible earlier.

### 14.14 HN Language And Intermediate Format

HNChain should not require one source language forever.

The preferred long-term direction is one canonical intermediate format:

```text
Rust-like language
Go-like language
Kotlin
TypeScript subset
HNScript
  -> HN Bytecode
  -> HNVM
```

This would allow multiple developer-facing languages to compile into the same
verified execution target.

However, language support must be added carefully. Every supported language
requires compiler maintenance, security review, debugging tools, documentation,
package management, and reproducible builds.

HNScript or another native language may be designed later as a safety-oriented
contract language, but HN Bytecode and HNVM semantics must come first.

## Chapter XVI: P2P Network

### 15.1 Network Purpose

The HNChain P2P layer is responsible for:

- discovering nodes
- transmitting transactions
- transmitting blocks
- transmitting consensus messages
- synchronizing state
- recovering after disconnection
- protecting the network from overload

If the P2P layer performs poorly, the chain cannot be fast or reliable even if
consensus, storage, and execution are well designed.

### 15.2 Layered Architecture

HNChain networking should be layered:

```text
Application
  -> RPC Layer
  -> P2P Layer
  -> Transport
  -> Internet
```

Boundary rules:

- Application logic defines protocol objects.
- RPC exposes external APIs.
- P2P defines peer behavior, message routing, gossip, discovery, and sync.
- Transport moves bytes between peers.
- Consensus validity is not defined by transport behavior.

### 15.3 Transport Independence

HNChain should not bind the protocol permanently to one transport.

The network stack should use a transport abstraction:

```text
TCP
QUIC
future transport
  -> HN Transport API
```

UDP may be used only through a protocol that defines reliability, congestion
control, authentication, encryption, and anti-amplification behavior.

Transport abstraction makes long-term evolution possible, but it must not hide
security-critical behavior. Connection identity, encryption, backpressure,
timeouts, and flow control must remain explicit.

### 15.4 Node Discovery

A new node starts with no local peer knowledge.

Conceptual boot flow:

```text
HNNode
  -> Bootstrap
  -> Peer List
  -> Discovery
  -> Network
```

Bootstrap nodes are only an entry point. The network must not depend on
bootstrap nodes after peer discovery succeeds.

The discovery specification must define:

- bootstrap record format
- peer identity verification
- address advertisement rules
- peer exchange limits
- eclipse-attack resistance
- private network and testnet behavior
- peer eviction behavior

### 15.5 Peer Table

Each node maintains a peer table.

Candidate peer metadata:

- node identifier
- network addresses
- supported protocol versions
- capabilities
- observed latency
- last successful response time
- failed request count
- useful data served
- disconnect history
- local reputation score

Peer table data is local node policy. It must not affect consensus validity.

### 15.6 Node Reputation

HNChain should investigate local node reputation.

Example signals:

- timely responses increase priority
- invalid data decreases priority
- repeated disconnects decrease priority
- useful sync service increases priority
- malformed packets decrease priority

Reputation must be treated carefully.

Security risks:

- Sybil nodes can try to farm reputation.
- Attackers can attempt reputation poisoning.
- Local scoring can reduce peer diversity.
- Over-trusting high-reputation peers can increase eclipse risk.

Node reputation should be local, bounded, decay over time, and never become a
consensus input.

### 15.7 Gossip

HNChain should use gossip for transactions, blocks, consensus messages, and
network metadata where appropriate.

To reduce bandwidth, the network should prefer announce-then-request flows for
large objects:

```text
Object Hash
  -> Need?
  -> Download Object
  -> Validate
  -> Relay
```

The gossip specification must define:

- message identifiers
- duplicate suppression
- fanout
- peer selection
- rate limits
- validation before relay
- behavior for invalid announcements
- privacy considerations

### 15.8 Block Propagation

HNChain should investigate staged block propagation:

```text
Leader
  -> Header
  -> Preliminary Validation
  -> Body
  -> Full Validation
  -> Finality
```

Header-first propagation may reduce latency by allowing nodes to prepare before
the full block body arrives.

Security warning:

Header-first propagation must not allow invalid headers to trigger expensive
work or resource exhaustion. Preliminary validation must be cheap and bounded.

### 15.9 Compression

P2P messages may use compression when beneficial.

Compression must be negotiated through capabilities and must define:

- supported algorithms
- maximum decompressed size
- compression ratio limits
- CPU cost limits
- per-message eligibility
- fallback behavior

Security warning:

Compression can create decompression bombs and CPU exhaustion. Nodes must verify
declared sizes and enforce limits before allocation and decompression.

### 15.10 Parallel Channels

HNChain should separate traffic classes:

```text
Channel A -> Transactions
Channel B -> Blocks
Channel C -> Consensus
Channel D -> Sync
```

Separate channels reduce the chance that bulk sync or transaction gossip blocks
time-sensitive consensus messages.

The networking specification must define channel isolation, backpressure,
fairness, queue limits, and failure behavior.

### 15.11 Message Priority

Not all messages have equal priority.

Initial priority direction:

```text
Consensus
  -> Blocks
  -> Transactions
  -> Sync
```

Priority rules must prevent low-priority traffic from starving critical
consensus messages.

They must also prevent attackers from marking spam as high priority. Message
priority is assigned by protocol message type, not by peer preference.

### 15.12 DDoS And Resource Protection

Every inbound packet or message must pass cheap checks before expensive work.

Conceptual validation path:

```text
Packet
  -> Framing
  -> Size Limits
  -> Version Check
  -> Authentication
  -> Cheap Validation
  -> Full Validation
  -> Processing
```

The P2P layer must enforce:

- connection limits
- per-peer rate limits
- message size limits
- decompressed size limits
- authentication before expensive processing
- validation before relay
- request budgets
- ban or cooldown rules
- anti-amplification rules

### 15.13 Synchronization

Full history may become large.

HNChain should support several synchronization modes:

```text
Full Verification
  Genesis -> ... -> Current

Fast Sync
  Snapshot -> Verification -> Continue

Light Verification
  Headers / Proofs -> Verified Views
```

Full verification provides the strongest independent validation, but it may be
slow for new nodes.

Snapshot-based sync is faster, but it requires state root verification,
checkpoint rules, proof formats, and peer diversity.

Fast sync must not become blind trust.

### 15.14 HN Relay Research Direction

HNChain should investigate a non-consensus relay node role:

```text
Validator
  -> HN Relay
  -> World
```

HN Relay nodes would focus on fast data propagation. They do not vote, finalize
blocks, or define consensus validity.

Potential benefits:

- lower bandwidth load on validators
- faster block and transaction propagation
- better support for geographically diverse peers
- specialized networking without changing validator rules

Risks:

- relay centralization
- censorship at relay layer
- accidental dependency on a small relay set
- traffic analysis
- incentive design complexity

Architectural rule:

HNChain must remain functional without relay nodes. Relays may improve
performance, but they must not become required trust anchors.

### 15.15 Encryption

Peer-to-peer communication should be encrypted and authenticated.

Encryption reduces passive traffic analysis and active message tampering.

The network specification must define:

- peer identity
- handshake
- key agreement
- session keys
- replay protection
- protocol version binding
- connection resumption behavior
- downgrade resistance

Encryption does not replace consensus validation. A valid encrypted channel can
still carry invalid protocol data.

### 15.16 Protocol Versioning And Capabilities

Every network session must negotiate protocol version and capabilities.

Conceptual handshake:

```text
Protocol Version
  -> Capabilities
  -> Compression
  -> Encryption
  -> Channel Set
```

Candidate capabilities:

- supports QUIC
- supports compression
- supports fast sync
- supports snapshot service
- supports VM v2 relay data
- supports post-quantum signature gossip
- supports archival data service

Compatibility rule:

Capability negotiation may change transport, compression, sync mode, and
optional service behavior. It must not change consensus validity rules.

If peers do not share a required capability, they must fail safely or use a
defined fallback.

### 15.17 P2P Specification Requirements

Before implementation, HNChain must define:

- network packet format
- message type registry
- handshake protocol
- peer identity format
- transport abstraction
- channel model
- gossip protocol
- block propagation protocol
- transaction propagation protocol
- consensus message propagation
- sync protocols
- compression profiles
- encryption profiles
- capability negotiation
- rate limits and resource limits
- error codes and disconnect reasons
- test vectors and interoperability tests

## Chapter XVII: Storage Engine

### 16.1 Why Storage Matters

Most users never think about where blockchain data is stored.

Storage design determines:

- how much disk space the network needs after ten years
- how quickly a new node can start
- how quickly historical and account queries execute
- how much memory validators need
- how easy it is to archive and restore history
- how safely nodes recover from local data corruption

Storage is therefore a foundation of performance, decentralization, and
long-term maintainability.

### 16.2 HNStorage Requirements

HNStorage must provide:

- high write throughput
- fast lookup
- data corruption detection
- scalability
- archival support
- snapshot support
- deterministic state commitments
- replaceable backend implementation

Important boundary:

Different hardware may have different performance, but the same canonical state
must produce the same state root on every compliant node.

### 16.3 Layered Storage Architecture

HNChain storage should be layered:

```text
Application State
  -> State Database
  -> Block Database
  -> Transaction Database
  -> Storage Engine
  -> Disk
```

Boundary rules:

- Application state defines protocol meaning.
- State database owns current state views and authenticated state commitments.
- Block database stores finalized and pending block data.
- Transaction database stores transaction bodies, receipts, and indexes.
- Storage engine persists bytes and indexes.
- Disk and filesystem behavior must not define consensus semantics.

### 16.4 Data Separation

HNChain should not store all data in one undifferentiated database namespace.

Conceptual storage spaces:

```text
Blocks/
Transactions/
Accounts/
Contracts/
Metadata/
Snapshots/
Logs/
```

Each data type may have its own indexing, retention, compression, and archival
rules.

Physical layout remains an implementation detail as long as canonical state,
proofs, and compatibility requirements are preserved.

### 16.5 Immutable Finalized Data

After a block is finalized, its canonical contents become immutable under
protocol assumptions.

```text
Block
  -> Finalized
  -> Immutable
```

Immutable block data simplifies historical verification, archival packaging,
replication, and corruption detection.

Any change to finalized block contents must be detected through block hashes,
state roots, receipts roots, and archival verification.

### 16.6 State Database

The state database is the most frequently updated storage layer.

It contains or indexes:

- balances
- account metadata
- account permissions
- smart contract state
- token state
- NFT state
- DAO state
- protocol service structures
- validator state

The state database must expose controlled interfaces to the state transition
engine and HNVM.

HNVM must not write directly to the physical storage backend.

### 16.7 Snapshot System

Snapshots can reduce new-node synchronization time.

Conceptual flow:

```text
Genesis
  -> Height 100000
  -> Height 200000
  -> Height 300000
  -> Snapshot
```

A new node may download a verified snapshot, check its cryptographic commitment,
and then sync blocks after the snapshot height.

Snapshot specification must define:

- snapshot height
- state root
- block hash
- chain identifier
- format version
- chunking
- checksum
- signatures or attestations, if used
- proof format
- restore procedure
- corruption handling

Fast snapshot sync must not become blind trust.

### 16.8 Node Storage Profiles

HNChain should support multiple node storage profiles.

```text
Full Node
  Genesis -> ... -> Current

Snapshot Node
  Verified Snapshot -> Current

Archive Node
  Everything -> Forever
```

Full nodes provide stronger independent verification.

Snapshot nodes reduce bootstrap time and storage requirements but depend on
verified snapshot and checkpoint rules.

Archive nodes preserve complete historical data and support long-term recovery,
explorer indexing, audits, and research.

The protocol must define which node profiles can validate, serve proofs, vote,
or participate in consensus.

### 16.9 Deduplication

HNChain may use deduplication for repeated data patterns.

Examples:

- repeated addresses
- repeated commitments
- repeated metadata fragments
- shared state substructures

Deduplication is a storage optimization. It must not change canonical bytes,
hashes, state roots, or proof semantics.

### 16.10 Storage API

Blockchain logic must be separated from the concrete database backend.

```text
Core
  -> Storage API
  -> Backend
```

This allows HNChain to use one storage backend initially and migrate or support
another backend later without rewriting protocol logic.

The Storage API must define:

- read operations
- write operations
- atomic batch behavior
- rollback behavior
- snapshot reads
- iteration order
- consistency guarantees
- corruption reporting
- pruning and archival hooks

Iteration order must be deterministic when used by consensus logic.

### 16.11 Integrity Verification

Every stored consensus object must be verifiable.

```text
Block
  -> Hash
  -> Verify
  -> OK or Corrupt
```

Integrity verification must cover:

- block bodies
- block headers
- transactions
- receipts
- state roots
- snapshots
- archive packages
- storage indexes where needed

Local database corruption must be detectable and recoverable where possible.

### 16.12 Authenticated State Structure

HNChain should use a Merkle-like authenticated data structure or another
well-analyzed commitment structure for state.

Candidate structures:

- classic Merkle tree
- Merkle Patricia Trie
- Sparse Merkle Tree
- Verkle Tree
- other authenticated structures after analysis

HNChain should not invent a replacement only for originality.

The decision must consider:

- update performance
- proof size
- implementation complexity
- light-client verification
- archival behavior
- parallel execution support
- state bloat
- compatibility with snapshots

This decision belongs in ADR-0007 State Tree.

### 16.13 HNState Research Direction

HNChain should investigate splitting state into independent domains.

Conceptual domains:

```text
Accounts
  -> Contracts
  -> Tokens
  -> NFT
  -> DAO
```

Each domain may have its own index and storage rules while still committing to a
single canonical state root or a formally defined state-root composition.

Benefits:

- clearer ownership of data
- targeted indexing
- easier pruning and archival policy
- possible parallel updates
- better query performance

Risks:

- more complex proof composition
- harder cross-domain transaction semantics
- migration complexity
- risk of inconsistent root calculation if composition is underspecified

### 16.14 Temporary Data Cleanup

Temporary data must not be stored forever.

Examples:

- mempool cache
- peer cache
- temporary execution cache
- sync staging data
- expired indexes

Conceptual flow:

```text
Temp Cache
  -> Expired
  -> Delete
```

This applies only to cache and service data, not to finalized blockchain data
unless pruning or archival rules explicitly allow it.

### 16.15 Verifiable Snapshots

A verifiable snapshot should include:

```text
State Root
  -> Block Height
  -> Block Hash
  -> Format Version
  -> Checksum
  -> Signature or Attestation
```

Nodes must verify that a snapshot corresponds to a valid chain state before
using it.

Snapshot trust assumptions must be explicit.

### 16.16 HN Archive Protocol Research Direction

After decades, chain history may become very large.

HNChain should investigate a separate archival protocol:

```text
Old Blocks
  -> Archive Package
  -> Distributed Storage
  -> Verification
```

Archive data may be stored outside the hot validator path while remaining
verifiable and recoverable.

The archival protocol must define:

- package format
- chunking
- checksums
- block range commitments
- retrieval protocol
- proof verification
- retention incentives
- corruption recovery

Archive storage must not become a hidden central dependency.

### 16.17 Long-Term Storage Goals

HNStorage has three strategic goals:

- allow a new node to become useful in minutes where security mode permits it
- use disk space efficiently
- remain independent from one storage technology

The network must scale without requiring hardware needs to grow directly in
proportion to chain age for every node role.

These goals require snapshots, pruning, archival roles, storage accounting, and
authenticated state proofs to be designed together.

## Chapter XVIII: Cryptography

### 17.1 Core Rule

HNChain must not create custom cryptographic algorithms without years of open
analysis by the wider cryptographic community.

Most new cryptographic algorithms fail because they have not survived long-term
public review. HNChain should use well-studied standards while preserving an
architecture that can migrate to new standards in the future.

### 17.2 Foundation Of Trust

Cryptography protects:

- every coin
- every block
- every transaction
- every smart contract
- every state commitment
- every validator vote
- every secure peer connection

If the cryptographic layer fails, the blockchain fails.

For that reason, HNChain cryptography must be simple, open, auditable, modular,
and replaceable.

### 17.3 Cryptographic Uses

HNChain uses cryptography for:

- address derivation
- digital signatures
- identity verification
- hash computation
- state commitments
- proof verification
- secure node communication
- wallet recovery and key management

Wallet recovery and key storage are important, but they belong to wallet
security specifications and must not silently define consensus behavior.

### 17.4 Signing Flow

Conceptual transaction signing flow:

```text
Wallet
  -> Private Key
  -> Digital Signature
  -> Transaction
  -> Validator
  -> Verification
```

The private key must not leave the user's secure signing environment.

The network receives only the signature, public data needed for verification,
and the signed transaction payload.

### 17.5 Keys And Addresses

Conceptual relationship:

```text
Private Key
  -> Public Key
  -> Key Descriptor
  -> Address
```

An address is derived or bound according to protocol rules. It is not simply a
raw public key.

The user-facing address format should preserve a recognizable HNChain prefix,
such as:

```text
hn1x7da9...
```

The exact prefix, casing rules, checksum, and payload format are defined by the
address format specification.

Reference:

- `docs/adr/ADR-0003-address-format.md`

### 17.6 Signature Verification

Every transaction requiring account authorization must include a valid signature
or approved authorization proof.

Conceptual verification:

```text
Signature
  -> Public Key or Key Reference
  -> Verification Context
  -> Verify
  -> Accept or Reject
```

If verification fails, the transaction is rejected before expensive execution.

Every signature must bind to a context that prevents replay across networks,
chains, object types, and protocol versions.

### 17.7 Hash Functions

Hash functions are used for:

- block identifiers
- transaction identifiers
- Merkle-like trees
- state verification
- file and snapshot integrity
- protocol registries

HNChain must choose modern, openly specified, widely analyzed hash functions.

Candidate algorithms include SHA-256, SHA-3-family functions, SHAKE, BLAKE3, or
other well-reviewed algorithms. The choice must be made through hash profiles,
domain separation, benchmarking, and security review.

Reference:

- `docs/adr/ADR-0005-hash-algorithms.md`

### 17.8 Seed Phrases And Wallet Recovery

Wallets may use mnemonic seed phrases for recovery.

Conceptual flow:

```text
24 words
  -> Seed
  -> Private Key
  -> Wallet
```

HNChain should prefer established wallet recovery standards, such as BIP-39 or
modern alternatives, unless a dedicated wallet security review justifies a
different approach.

Seed phrase design is not consensus logic. It belongs to wallet specifications.

### 17.9 Hardware Wallets

HNChain should support hardware wallets from the beginning.

Hardware wallets allow users to sign transactions without exposing private keys
to a general-purpose computer.

The wallet specification must define:

- signing payload display
- address confirmation
- network identification
- transaction simulation data
- firmware compatibility
- supported signature algorithms
- recovery behavior

### 17.10 Multi-Signature

Some operations require multiple independent approvals.

Example:

```text
3 Keys
  -> 2 Signatures
  -> Transaction Valid
```

Multi-signature is useful for:

- corporate accounts
- DAO treasuries
- family custody
- development funds
- validator operations
- bridge operators

The multi-signature model must be specified before activation. It must define
threshold rules, key roles, replay protection, fee behavior, and signature
aggregation or verification costs.

### 17.11 Time Locks

HNChain may support time-locked or condition-locked transactions.

Conceptual flow:

```text
Created
  -> Locked
  -> Unlock Condition Satisfied
  -> Spend
```

Time locks must use consensus-defined time or height semantics, not local system
time.

The transaction specification must define whether locks are based on block
height, finalized checkpoint, consensus timestamp, epoch, or another protocol
measure.

### 17.12 Quantum Migration

Most current blockchain signatures are not designed to resist large-scale
cryptographically relevant quantum computers.

HNChain must therefore be designed for cryptographic migration.

The protocol must support:

- algorithm identifiers
- versioned key descriptors
- versioned signature envelopes
- algorithm lifecycle states
- historical verification rules
- post-quantum or hybrid migration paths

The goal is to upgrade cryptography without creating a new blockchain, while
preserving historical verification where possible.

### 17.13 Crypto API

HNChain core logic should depend on a cryptographic interface, not direct calls
to one algorithm implementation.

```text
Core
  -> Crypto API
  -> Algorithms
```

The Crypto API must define:

- key descriptor parsing
- signature verification
- hash profile resolution
- domain-separated hashing
- algorithm lifecycle checks
- test vector execution
- error taxonomy

The Crypto API must not hide consensus-critical behavior. Algorithms, encodings,
and rejection rules remain protocol-defined.

### 17.14 HNCrypto Registry

HNChain should maintain a registry for cryptographic algorithms and profiles.

Conceptual examples:

```text
HNCR-001 -> Digital Signature -> Supported
HNCR-002 -> Hash Function     -> Supported
```

The registry should define:

- algorithm identifier
- algorithm type
- supported roles
- lifecycle state
- canonical encoding
- verification rules
- test vectors
- activation and deprecation rules

The registry must be governed by protocol rules, not local node preference.

### 17.15 Implementation Security

Even a strong algorithm can become unsafe through a bad implementation.

HNChain cryptographic implementation should require:

- well-reviewed cryptographic libraries
- independent audits
- known-answer tests
- negative test vectors
- side-channel resistance where secret material is processed
- constant-time behavior where required
- reproducible builds
- dependency review
- fuzzing and differential testing

Custom cryptographic primitives are rejected unless accepted through a dedicated
ADR and external review.

### 17.16 Long-Term Strategy

HNChain's cryptographic strategy is:

```text
Cryptography must be replaceable, not frozen.
```

If the global cryptographic community adopts new signature or hash standards in
the future, HNChain should be able to migrate through a protocol upgrade while
preserving compatibility with historical data where possible.

### 17.17 Chapter Summary

HNChain cryptography must be:

- based on reviewed standards
- modular
- extensible
- compatible with hardware wallets
- ready for post-quantum migration
- transparent to independent audit

## Chapter XIX: Accounts And Wallet Architecture

### 18.1 Account Philosophy

In many blockchains, an account is mainly an address with a balance.

HNChain uses a broader model.

An HNChain account is a digital identity within the network. It may own or
control:

- HNC
- user-defined tokens
- NFTs
- smart contracts
- governance rights
- DAO roles
- domain names
- digital certificates
- protocol permissions

This does not mean every account must expose identity data. The base account can
remain pseudonymous. Additional identity features must be explicit and optional.

### 18.2 Account Structure

Conceptual account structure:

```text
HN Account
  -> Address
  -> Identity / Key Descriptors
  -> Account Type
  -> Balance
  -> Nonce
  -> Permissions
  -> Assets
  -> Metadata
  -> Lifecycle
  -> Version
```

HNChain should not model an account as a single raw public key. Accounts bind to
cryptographic identity through versioned key descriptors and permission rules.

This preserves support for key rotation, multisignature, hardware wallets,
session keys, and post-quantum migration.

### 18.3 Account Types

HNChain should support multiple account types.

Initial conceptual types:

```text
Standard Account
Validator Account
Smart Contract Account
System Account
```

Account type semantics must be specified before implementation.

### 18.4 Standard Account

A standard account represents a regular user or organization.

It may:

- hold HNC
- sign transactions
- interact with smart contracts
- hold assets
- delegate permissions
- use wallet recovery mechanisms

### 18.5 Validator Account

A validator account represents validator participation.

It may contain or reference:

- stake amount
- validator status
- consensus key binding
- network key binding
- reward destination
- slashing state
- performance metadata
- participation parameters

Validator network reputation, if introduced, must not be confused with
consensus voting weight unless a consensus specification explicitly defines that
relationship.

### 18.6 Smart Contract Account

A smart contract account has:

- its own address
- its own persistent state
- code or code commitment
- balance, if allowed
- permissions
- lifecycle state

Representing contracts as accounts creates a uniform model, but contract account
rules must define deployment, upgrades, calls, storage ownership, and destruction
semantics.

### 18.7 System Account

System accounts represent protocol-owned services.

Examples:

- fee accounting
- staking module
- slashing module
- treasury or development fund, if accepted
- governance module
- bridge registry
- name service registry

System accounts must be reserved by genesis or governance-controlled activation
rules.

### 18.8 Address Format

User-facing addresses should be visually recognizable as HNChain addresses.

Conceptual example:

```text
hn1xxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

The canonical address format is defined by ADR-0003. The string prefix is a
display and UX feature, not the sole network-separation mechanism.

### 18.9 Human-Readable Names

HNChain may support human-readable names through a separate naming or DNS-like
service.

Examples:

```text
roman.hn
wallet.high
```

Human-readable names must not replace canonical addresses in consensus.

Name resolution must define ownership, expiration, renewal, disputes, privacy,
and phishing protections.

### 18.10 Multi-Account Wallet

One wallet may manage many accounts.

Conceptual structure:

```text
HN Wallet
  -> Personal
  -> Business
  -> Validator
  -> Cold Storage
```

This allows one wallet application to support different user scenarios without
creating separate applications for each role.

### 18.11 Recovery

Wallet recovery may begin from a mnemonic or another approved recovery method.

Conceptual flow:

```text
Seed Phrase
  -> Private Keys
  -> Wallet
  -> Accounts
```

Wallets should be able to discover related accounts according to documented
derivation and account discovery rules.

Recovery is security-sensitive and must be specified with test vectors before
production use.

### 18.12 Permission Model

HNChain should support permissions beyond:

```text
Private Key = Full Access
```

Conceptual roles:

```text
Owner
  -> Administrator
  -> Operator
  -> Viewer
```

Permissions are useful for organizations, validators, DAO operations, and
controlled automation.

The permission model must define:

- role creation
- role revocation
- permission scope
- key binding
- delegation
- recovery
- auditability
- transaction signing requirements
- conflict resolution

### 18.13 Spending Limits

HNChain may support spending limits.

Example:

```text
<= 100 HNC
  -> Allowed by session or operator policy

> 100 HNC
  -> Requires stronger approval
```

Spending limits can reduce losses after device compromise.

They must be enforced by consensus state transition rules if they affect
transaction validity. Wallet-only spending limits improve UX but must not be
treated as protocol guarantees.

### 18.14 Delegation

Users may delegate limited rights.

Example:

```text
Owner
  -> Delegate
  -> Vote
```

Delegation should allow specific capabilities without granting full control over
funds.

Delegation must define scope, expiration, revocation, replay protection, and
audit trail.

### 18.15 Session Keys

Session keys allow temporary authorization for limited actions.

Conceptual flow:

```text
Main Wallet
  -> Temporary Session Key
  -> Expires
```

Session keys can improve dApp UX by reducing repeated use of the primary key.

Security requirements:

- explicit expiration
- limited scope
- revocation
- spending limits
- device binding, if used
- clear signing context
- protection against phishing approvals

### 18.16 Device Binding

HNChain wallets may support trusted device binding.

Conceptual model:

```text
Wallet
  -> Laptop
  -> Phone
  -> Hardware Wallet
```

Each device may receive its own key or identifier and may be revoked without
rotating every account key.

Device binding belongs to wallet and account permission specifications. It must
not weaken self-custody or create hidden custodial dependencies.

### 18.17 Emergency Lock

HNChain may support emergency protection flows.

Conceptual flow:

```text
Emergency
  -> Freeze
  -> Recover
```

Emergency lock can help after suspected compromise, but it is dangerous if
misdesigned.

Risks:

- attacker-triggered account freeze
- governance or service abuse
- denial of access to legitimate owners
- conflict with self-custody expectations
- unclear recovery authority

Emergency mechanisms require a dedicated specification before activation.

### 18.18 HN Wallet

HNChain should provide an official wallet while remaining open to independent
wallets.

Target platforms:

- Windows
- Linux
- macOS
- Android
- iOS
- Web

HN Wallet should support:

- HNC
- user-defined tokens
- NFTs
- staking
- validator management
- DAO interactions
- human-readable names
- transaction history
- hardware wallets
- multisignature

### 18.19 Wallet API

Wallets interact with the network through stable APIs.

```text
Wallet
  -> RPC
  -> Node
  -> Blockchain
```

Stable wallet APIs allow alternative clients to exist without changing the
protocol.

Wallet APIs must expose signing payloads, simulation results, fees, network
identity, account permissions, and transaction status in documented forms.

### 18.20 HN Identity Research Direction

HN Identity is a possible voluntary identity layer.

Conceptual model:

```text
HN Account
  -> HN Identity
  -> Verified Profile
```

Separation is mandatory:

- the base account remains pseudonymous
- identity verification is voluntary
- identity data is used only where a service or law requires it
- users must be able to use the base protocol without identity disclosure

HN Identity must define privacy, revocation, attestations, data minimization,
and jurisdictional risk before implementation.

### 18.21 Chapter Summary

HNChain accounts should be:

- extensible
- secure
- role-aware
- usable for regular users
- usable for organizations
- compatible with hardware wallets
- compatible with future identity services
- ready for ecosystem growth

## Chapter XX: RPC, API And SDK

### 19.1 Minimal Protocol, Large Ecosystem

HNChain should follow a simple architectural principle:

```text
The protocol should be minimal. The ecosystem can be large.
```

The base protocol should define only what must be shared for consensus,
security, interoperability, and long-term compatibility.

Applications, SDKs, tools, libraries, portals, templates, and developer services
can grow around the protocol without expanding consensus scope unnecessarily.

### 19.2 Why APIs Matter

For many users, the blockchain is experienced through a wallet.

For developers, the blockchain is experienced through APIs.

If the API is inconsistent, poorly documented, or hard to test, developers will
choose another platform even if the underlying protocol is strong.

HNChain APIs are therefore an official contract between the network and
applications.

### 19.3 API Architecture

Conceptual architecture:

```text
Application
  -> HN SDK
  -> RPC Client
  -> RPC Gateway
  -> HN Node
  -> Blockchain
```

Applications should interact with HNChain through stable, documented, versioned
interfaces.

Boundary rule:

APIs expose protocol data and node services. They do not define consensus truth.

### 19.4 RPC Layer

Each node should expose consistent interfaces for:

```text
RPC
  -> Accounts
  -> Transactions
  -> Blocks
  -> Contracts
  -> Events
  -> Validators
  -> Network
```

RPC behavior must be specified so clients behave predictably across node
implementations.

### 19.5 JSON-RPC

HNChain should support JSON-RPC as an official interface.

Reasons:

- simple request and response model
- broad tooling support
- familiar to blockchain developers
- easy integration for wallets and scripts

JSON-RPC is not a consensus encoding. It is an external API representation.

### 19.6 REST API

HNChain should also support REST APIs for simple integrations, analytics, and
explorer workflows.

Examples:

```text
GET /block/latest
GET /account/{address}
GET /transaction/{hash}
```

REST responses must be versioned and documented.

### 19.7 WebSocket And Subscriptions

Some applications require real-time updates.

Examples:

```text
Wallet
  -> Subscribe
  -> New Block

Explorer
  -> Subscribe
  -> Transfer Event
```

Subscription APIs should avoid constant polling and provide documented event
delivery semantics.

The specification must define reconnection, missed events, replay windows,
ordering, backpressure, and rate limits.

### 19.8 Event Stream

HNChain should define a common event stream model.

Candidate event types:

- new block
- new transaction
- transaction finalized
- contract event
- transfer event
- validator status
- governance proposal
- network status

Events are indexed views over protocol data. Events must not create hidden
consensus semantics.

### 19.9 Graph API Research Direction

HNChain may investigate a standard query layer for indexed data.

Conceptual model:

```text
Application
  -> Graph Query
  -> Index
  -> Blockchain
```

This can reduce reliance on incompatible third-party indexers.

However, a graph query layer is an ecosystem component, not a required consensus
feature. It must be clearly separated from node validation.

### 19.10 SDKs

HNChain should provide official SDKs for:

- Rust
- Go
- TypeScript
- Python
- Java
- Swift
- Kotlin
- C#

SDKs must implement accepted specifications and include compatibility test
vectors.

SDKs must not define protocol behavior.

### 19.11 Unified SDK Interface

SDKs should expose similar concepts across languages.

Conceptual examples:

```text
wallet.transfer()
wallet.balance()
wallet.sign()
wallet.deploy()
```

The goal is not to force every language into identical syntax. The goal is to
make concepts, naming, error handling, and workflows consistent enough that a
developer can move between SDKs without relearning the protocol.

### 19.12 CLI

HNChain should provide an official command-line tool.

Candidate commands:

```text
hn init
hn build
hn test
hn deploy
hn wallet
hn validator
hn dev
```

The CLI is a developer and operator tool. It must call documented APIs and must
not contain hidden protocol behavior.

### 19.13 Local Node

Developers should be able to start a local development environment with one
command.

Conceptual command:

```text
hn dev
```

Conceptual services:

```text
Local Node
  -> Wallet
  -> RPC
  -> Explorer
  -> Faucet
```

Local development mode must be clearly separated from mainnet behavior.

### 19.14 Sandbox

Smart contracts should be executable in a safe local sandbox.

Conceptual flow:

```text
Sandbox
  -> Contract
  -> Execute
  -> Debug
```

Sandbox execution should help developers test contract behavior without risk to
real network state.

Sandbox behavior must document where it matches consensus execution and where it
is developer-only simulation.

### 19.15 Debug API

HNChain should provide debugging and analysis APIs for development and
operations.

Capabilities may include:

- state inspection
- call tracing
- resource metering reports
- execution profiling
- transaction simulation
- contract event inspection
- access graph visualization

Debug APIs must be protected where needed and must not be required for consensus
validation.

### 19.16 API Versioning

Every public API must be versioned.

Conceptual lifecycle:

```text
API v1
  -> API v2
  -> API v3
```

Old clients should continue working while their API version is supported.

The API specification must define:

- version negotiation
- deprecation policy
- compatibility windows
- error behavior
- feature discovery
- unsupported method behavior

### 19.17 Documentation First

HNChain API development follows:

```text
Documentation -> Specification -> Implementation -> Tests
```

Every RPC method must define:

- description
- parameters
- request example
- response example
- error codes
- version
- permissions or access requirements
- rate-limit behavior
- usage recommendations

APIs without documentation are not accepted as public APIs.

### 19.18 HN Package Manager Research Direction

HNChain may investigate an ecosystem package manager.

Conceptual examples:

```text
hn install token
  -> Verified Package
  -> Ready

hn install dao
  -> Official DAO Library

hn install nft-marketplace
```

This could help developers avoid copying unaudited contract code from random
sources.

The package manager should be treated as an ecosystem tool, not a consensus
component.

Security requirements would include:

- package signing
- reproducible builds
- source provenance
- version pinning
- vulnerability advisories
- audit metadata
- dependency review
- namespace governance

### 19.19 Developer Portal

HNChain should provide a unified developer portal.

The portal should include:

- documentation
- SDKs
- examples
- project templates
- sandbox tools
- testnet access
- library catalog
- security recommendations
- compatibility test vectors

The developer portal improves adoption, but it must not become the only way to
use or understand the protocol.

### 19.20 Chapter Summary

The HNChain developer ecosystem should make the path from idea to first working
contract short and predictable.

This requires:

- unified APIs
- official SDKs
- a practical CLI
- local development tools
- clear documentation
- testing and debugging tools
- verified ecosystem libraries
- stable versioning

## Chapter XXI: Explorer

HNChain should provide an official open-source explorer.

Explorer views:

```text
Blocks
Transactions
Validators
Accounts
Contracts
Governance
Network Health
```

The explorer is not a source of consensus truth. It indexes and presents data
derived from nodes and archival services.

## Chapter XXII: Wallet

HNChain should provide official wallet implementations for:

- Windows
- Linux
- macOS
- Android
- iOS
- Web

The wallet security specification must define:

- key generation
- key storage
- hardware wallet integration
- signing UX
- transaction simulation
- address display
- recovery
- phishing resistance
- network identification
- secure update policy

An official wallet is important, but the protocol must remain open to
independent wallets.

## Chapter XXIII: Governance And Protocol Evolution

### 22.1 Why Governance Exists

Writing blockchain software is difficult.

Designing a network that can evolve for twenty years without splitting into
incompatible forks is harder.

HNChain starts from one governance philosophy:

```text
The protocol belongs to its rules.
```

This does not mean there are no developers, companies, foundations, validators,
or communities. It means protocol changes must pass through transparent,
formalized procedures instead of depending on one person or one organization.

### 22.2 Core Governance Principle

HNChain should not be governed by code alone, social pressure alone, token
wealth alone, or company control alone.

The network should be governed through a formal protocol change process.

```text
Formal Change Process
  -> Specification
  -> Review
  -> Tests
  -> Activation
```

Process is part of protocol safety.

### 22.3 HNIP

HNChain uses HNIP:

```text
HNIP = HN Improvement Proposal
```

Every significant change begins with an HNIP or a linked ADR/specification.

HNIPs are used for:

- protocol changes
- VM changes
- networking changes
- token and contract standards
- governance changes
- informational ecosystem proposals

### 22.4 HNIP Lifecycle

HNIP lifecycle:

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

Consensus-critical changes must not skip review, testing, or activation stages.

### 22.5 HNIP Categories

Initial HNIP categories:

```text
HNIP-C -> Core
HNIP-V -> VM
HNIP-N -> Networking
HNIP-S -> Standards
HNIP-I -> Informational
```

Categories help reviewers route proposals to the right expertise.

### 22.6 Specification First

HNChain follows:

```text
HNIP
  -> Specification
  -> Discussion
  -> Code
```

Code should not be accepted as protocol behavior before an accepted
specification exists.

For consensus-critical changes, the stronger rule is:

```text
Specification
  -> Reference Tests
  -> Implementation
```

If two independent implementations pass the same tests, the specification is
much more likely to be precise enough.

### 22.7 Security Review And Audit

Critical changes require independent review.

Conceptual flow:

```text
Implementation
  -> Audit
  -> Fixes
  -> Merge
```

Audit does not guarantee safety, but it reduces the probability of severe
failures.

Security-sensitive changes include:

- consensus changes
- cryptographic changes
- VM changes
- state transition changes
- wallet signing changes
- bridge changes
- governance activation changes

### 22.8 Test Networks

Protocol changes should move through staged environments:

```text
Development
  -> Devnet
  -> Testnet
  -> Mainnet
```

Mainnet activation should happen only after documented tests, compatibility
results, security review, and rollback or recovery planning.

### 22.9 Compatibility

HNChain should prioritize backward compatibility.

If backward compatibility is impossible, the proposal must include:

- reason for incompatibility
- affected components
- migration plan
- support window
- activation schedule
- risks
- rollback or recovery considerations

Backward-incompatible changes must not be hidden inside minor updates.

### 22.10 Transparency

Each HNIP should contain:

```text
Problem
  -> Motivation
  -> Specification
  -> Security
  -> Compatibility
  -> Reference Implementation
```

This keeps discussion technical, reproducible, and reviewable.

### 22.11 Voting

Governance voting is a difficult design problem.

HNChain should not assume that decision-making is simply:

```text
1 HNC = 1 vote
```

Possible participants in a multi-layer governance process:

- validator operators
- client developers
- token holders
- independent researchers
- infrastructure providers
- application developers
- security auditors

The exact voting mechanism requires separate analysis of capture risk,
plutocracy, Sybil resistance, apathy, bribery, delegation, and emergency powers.

### 22.12 Emergency Changes

Critical vulnerabilities require an official emergency process.

Conceptual flow:

```text
Critical Bug
  -> Emergency Review
  -> Patch
  -> Rapid Deployment
  -> Disclosure
```

Emergency criteria must be clear.

Emergency power must be limited, auditable, and followed by public explanation
once disclosure is safe.

### 22.13 Protocol Versioning

HNChain protocol versions follow SemVer-style meaning.

Conceptual sequence:

```text
HNChain 1.0
  -> 1.1
  -> 1.2
  -> 2.0
```

Major versions indicate incompatible protocol changes.

Minor versions may add backward-compatible functionality.

Patch versions may clarify behavior or fix compatible implementation issues.

Exact versioning rules must be defined in protocol release specifications.

### 22.14 Reference Client

HNChain may provide an official reference client.

The reference client is not the protocol.

Any developer should be able to build an independent client if it conforms to
the accepted specifications and passes compatibility tests.

This reduces dependency on one software product and improves long-term
resilience.

### 22.15 HN Compatibility Tests

HNChain should maintain a public compatibility suite.

```text
Client
  -> Compatibility Suite
  -> PASS or FAIL
```

The suite should include:

- serialization vectors
- hash vectors
- signature vectors
- transaction validation vectors
- state transition vectors
- block validation vectors
- P2P packet vectors
- RPC compatibility tests
- VM execution vectors

Compatibility tests make independent implementations realistic.

### 22.16 Long-Term Minimalism

Every proposed core feature must answer:

- What problem does it solve?
- Why can it not be implemented at the application layer?
- Is the added protocol complexity worth the cost?

If a proposal cannot answer these questions, it should not enter the core
protocol.

### 22.17 HN Constitution

HNChain should maintain an HN Constitution.

Reference:

- `docs/governance/constitution.md`

This is not a legal document. It is a set of stable protocol principles.

Initial articles:

- Open Protocol
- Security First
- Specification First
- Backward Compatibility
- Transparency
- Minimal Core
- Independent Implementations

Changing the constitution should be significantly harder than changing an
ordinary specification.

### 22.18 Chapter Summary

HNChain protocol evolution should follow:

- specification before code
- open discussion
- mandatory testing
- independent implementations
- compatibility as a priority
- transparent decision procedures
- minimal protocol core

## Chapter XXIV: Security Model And Threat Analysis

### 23.1 Introduction

HNChain security is a continuous process.

It is not possible to create a system that no one can ever attack.

It is possible to design a system where the cost of successful attacks is high,
where failures are detectable, where damage is limited, and where recovery
procedures are defined.

HNChain's security model is based on this principle:

```text
Security is not a feature.
Security is architecture.
```

Security must be present in consensus, networking, cryptography, storage,
wallets, smart contracts, governance, and release engineering.

### 23.2 Trust Model

HNChain must not require trust in:

- one company
- one developer
- one validator
- one data center
- one client implementation
- one wallet
- one infrastructure provider

Trust is built through protocol rules, deterministic execution, independent
verification, open specifications, and compatibility tests.

### 23.3 Threat Categories

HNChain analyzes threats across several categories:

- consensus
- network
- cryptography
- storage
- wallet
- smart contracts
- supply chain
- human factor

Each category requires its own detailed threat model before production
implementation.

### 23.4 Consensus Threats

If an attacker tries to break consensus, the network should:

- detect protocol violations
- minimize damage
- preserve a consistent finalized state while security assumptions hold
- produce evidence where possible
- avoid unsafe recovery procedures

Concrete scenarios depend on the final consensus algorithm.

The consensus specification must analyze byzantine validators, equivocation,
censorship, invalid proposals, validator downtime, network partitions, and
finality failures.

### 23.5 Double Spending

Double spending attempts to spend the same funds twice.

Conceptual scenario:

```text
Wallet -> TX A -> Merchant
Wallet -> TX B -> Network
```

After finalization, conflicting transactions must not both be accepted.

The transaction format, nonce model, mempool policy, consensus finality, and
state transition rules must work together to prevent double spending.

### 23.6 Replay Attacks

A replay attack reuses a valid transaction or signature in an unintended
context.

Transactions and signatures must bind to:

- nonce or sequence state
- network identifier
- chain identifier
- protocol version
- account or key context
- transaction type
- signing purpose

Replay protection must be defined in the transaction format and cryptographic
identity specifications.

### 23.7 Sybil Attacks

In a Sybil attack, one entity creates many nodes or identities.

Conceptual scenario:

```text
Attacker
  -> 1000 Fake Nodes
```

HNChain must use technical and economic controls to make Sybil attacks less
effective.

Potential mitigations:

- peer diversity rules
- local reputation with decay
- validator staking or bonding, if accepted
- rate limits
- identity verification for validator roles
- eclipse-resistance in discovery

Sybil resistance must not rely on trusting one directory or one bootstrap
operator.

### 23.8 Eclipse Attacks

In an eclipse attack, an attacker isolates a victim node behind malicious peers.

Conceptual scenario:

```text
Victim
  -> Only Fake Peers
```

Mitigations to evaluate:

- diverse peer sources
- periodic peer rotation
- outbound and inbound peer diversity
- address group limits
- checkpoint verification
- cross-peer data comparison
- relay independence

The P2P specification must include eclipse-attack analysis.

### 23.9 DDoS And Resource Exhaustion

External inputs must pass cheap checks before expensive processing.

Conceptual flow:

```text
Packet
  -> Rate Limit
  -> Size Limits
  -> Validation
  -> Processing
```

HNChain must define resource limits for networking, RPC, transaction validation,
VM execution, storage reads, storage writes, sync requests, and decompression.

Attackers should not be able to force expensive work with cheap invalid input.

### 23.10 Key Compromise

If a private key is stolen, the protocol cannot know the original owner's intent
unless additional account rules exist.

The ecosystem should reduce key compromise risk through:

- hardware wallets
- multisignature
- spending limits
- session keys
- device revocation
- transaction simulation
- clear signing payload display
- emergency lock or recovery mechanisms, if accepted

These mechanisms require careful design to avoid creating new custody or
governance abuse risks.

### 23.11 Supply Chain Attacks

Attackers often target the development and release process instead of the
protocol itself.

HNChain should require:

- reproducible builds
- signed releases
- verifiable source code
- dependency review
- artifact verification
- release checksums
- protected build pipelines
- independent verification of binaries

Supply chain security is mandatory for node clients, wallets, SDKs, CLI tools,
and explorer infrastructure.

### 23.12 Smart Contract Attacks

Common smart contract vulnerability sources include:

- logic errors
- unsafe external calls
- integer overflow or underflow where possible
- incorrect permission checks
- reentrancy-like behavior
- resource metering mistakes
- undeclared state access
- unsafe upgrade mechanisms

HNChain should provide:

- static analysis
- secure development guidelines
- audited standard libraries
- deterministic sandbox execution
- contract simulation
- independent audit support

Tooling reduces risk, but it does not replace careful design and audit.

### 23.13 Client Vulnerabilities

Even a strong protocol can fail if client software has critical bugs.

HNChain clients require:

- automated tests
- fuzzing
- stress tests
- differential tests
- compatibility tests
- security audits
- memory safety review
- networking adversarial tests

Independent implementations are valuable, but only if they conform to the same
specifications and test vectors.

### 23.14 HN Security Levels

HNChain may define component maturity levels:

```text
Level 1 -> Experimental
Level 2 -> Reviewed
Level 3 -> Audited
Level 4 -> Mission Critical
```

These labels must not become marketing claims.

Formal criteria are required before assigning maturity levels to production
components.

### 23.15 Bug Bounty

HNChain should maintain an official responsible disclosure and bug bounty
program.

The program must define:

- scope
- severity classification
- reporting channel
- response timelines
- disclosure policy
- reward policy, if any
- safe harbor expectations
- duplicate report handling

Bug bounty rules must be public before mainnet.

### 23.16 Incident Response

Critical vulnerabilities require a defined process.

Conceptual flow:

```text
Report
  -> Verification
  -> Risk Assessment
  -> Patch
  -> Testing
  -> Release
  -> Disclosure
```

Clear incident response reduces chaos during emergencies.

The process must define who can triage reports, how emergency patches are
reviewed, how validators and node operators are notified, and how disclosure
happens after risk is reduced.

### 23.17 Independent Audits

Major releases should undergo independent security review.

Different audit teams should be used over time to reduce blind spots.

Audit targets include:

- consensus
- cryptography integration
- HNVM
- wallet signing
- P2P
- storage and snapshots
- governance activation
- bridge components, if any

### 23.18 Security Documentation

Every security-sensitive change must include:

```text
Threat Model
  -> Affected Components
  -> Mitigations
  -> Residual Risks
```

Residual risks must not be hidden.

This allows reviewers to understand both benefits and limitations.

### 23.19 Minimal Complexity

Every new feature increases attack surface.

HNChain should follow:

```text
If a function can be safely implemented outside the protocol core,
it should not become part of the protocol core.
```

This keeps the protocol smaller, easier to audit, and easier to implement
independently.

### 23.20 HN Security Principles

HNChain security principles:

```text
Trust the protocol, not the operator.
Verify, don't assume.
Minimal attack surface.
Defense in depth.
Specification before implementation.
Secure by default.
```

These principles should guide architecture, review, implementation, and
governance.

Reference:

- `docs/security/README.md`

### 23.21 Chapter Summary

HNChain security is layered:

- reliable consensus
- protected networking
- reviewed cryptography
- safe virtual machine
- strict specifications
- independent implementations
- continuous audit
- transparent incident response

## Chapter XXV: Token Standards And Digital Assets

### 24.1 Introduction

HNChain treats digital assets as a fundamental ecosystem layer.

Digital assets should have:

- understandable structure
- shared interfaces
- predictable behavior
- compatibility between wallets, explorers, exchanges, and applications
- documented security properties

HNChain introduces HNS:

```text
HNS = HN Standards
```

HNS documents describe behavior and interfaces. They should not force a single
implementation unless a protocol specification explicitly requires it.

### 24.2 Standard Architecture

Initial HNS families:

```text
HN Standards
  -> HNS-1 Fungible Tokens
  -> HNS-2 Non-Fungible Tokens
  -> HNS-3 Multi Asset
  -> HNS-4 Soulbound
  -> HNS-5 Governance
  -> HNS-6 Stable Assets
  -> HNS-7 Wrapped Assets
```

Each standard solves a specific problem and can evolve independently.

Compatibility rule:

HNS standards are ecosystem standards by default. They become protocol-level
rules only if accepted by an explicit protocol specification or HNIP.

### 24.3 HNS-1 Fungible Token Standard

HNS-1 defines a common interface for fungible assets.

Use cases:

- project tokens
- game currencies
- bonus points
- internal application assets
- tokenized claims

Candidate minimal interface:

```text
name()
symbol()
decimals()
totalSupply()
balanceOf()
transfer()
approve()
allowance()
transferFrom()
```

The final interface must define authorization, failure behavior, event emission,
metadata rules, and edge cases.

### 24.4 Fungible Token Metadata

Token metadata may include:

- name
- symbol
- decimals
- logo URI
- description
- website
- version

Metadata helps wallets and explorers display assets consistently.

Metadata must be bounded, versioned, and resistant to phishing or impersonation.

### 24.5 HNS-2 Non-Fungible Token Standard

HNS-2 defines a common interface for unique digital assets.

Use cases:

- art
- game items
- documents
- digital certificates
- collectibles

Candidate minimal interface:

```text
owner()
transfer()
metadata()
creator()
royalty()
```

The final standard must define ownership, transfer rules, metadata references,
creator attribution, royalty semantics, and event behavior.

### 24.6 NFT Metadata

NFT metadata may include:

- image
- animation
- video
- attributes
- license
- collection
- creator

The standard should define metadata format and integrity expectations. It should
not require large media files to be stored directly on-chain.

### 24.7 HNS-3 Multi Asset

HNS-3 defines a common interface for contracts that manage multiple asset types.

Example resources:

```text
Gold
Silver
Wood
Stone
Food
```

Multi-asset contracts can reduce overhead compared with deploying one contract
per asset type.

The standard must define asset identifiers, balances, transfers, approvals,
metadata, and event format across asset classes.

### 24.8 HNS-4 Soulbound Token

HNS-4 defines non-transferable token behavior.

Conceptual lifecycle:

```text
Issue
  -> Owner
  -> Locked
```

Use cases:

- diplomas
- professional certificates
- membership status
- achievements

Soulbound assets are privacy-sensitive. The standard must define revocation,
visibility, issuer authority, and user consent rules.

### 24.9 HNS-5 Governance Token

HNS-5 defines technical interfaces for governance-related assets.

The standard may define:

- voting power query
- delegation interface
- snapshot support
- proposal participation hooks
- event format

HNS-5 must not define one universal political model for HNChain.

The way vote weight is calculated belongs to each DAO or governance
specification.

### 24.10 HNS-6 Stable Asset Interface

HNS-6 is not a native stablecoin.

It is an interface that allows wallets, exchanges, and applications to recognize
assets intended to track stable value.

The standard may describe:

- collateral type
- stabilization mechanism
- issuer information, if applicable
- reserve audit links
- redemption rules
- risk disclosures

Security warning:

Stable asset labels can mislead users if not backed by transparent evidence.
The interface must distinguish claims from verified facts.

### 24.11 HNS-7 Wrapped Asset

HNS-7 defines representation of assets from other networks.

Examples:

```text
Bitcoin
  -> Wrapped
  -> HNChain

Ethereum
  -> Wrapped
  -> HNChain
```

The standard does not define the bridge mechanism itself.

It defines how wrapped assets are represented, identified, displayed, and
verified by ecosystem tools.

Bridge security belongs to a dedicated bridge specification.

### 24.12 Asset Manifest

Digital assets may publish an asset manifest.

Candidate properties:

```text
Transferable
Burnable
Mintable
Pausable
Governance
```

Wallets can use manifests to show users key asset behavior before interaction.

Manifest claims must be verifiable where possible and must not replace contract
validation.

### 24.13 Token Extensions

Base standards should remain small.

Optional behavior should be added through extensions:

```text
HNS-1
  -> Burn Extension
  -> Snapshot Extension
  -> Permit Extension
  -> Freeze Extension
```

This keeps the base interface simple while allowing richer functionality.

Extensions must declare compatibility, events, security considerations, and
interaction with base behavior.

### 24.14 Event Model

Asset standards should use a shared event model.

Candidate events:

- transfer
- mint
- burn
- approval
- metadata updated
- freeze
- unfreeze
- ownership changed

Consistent events make wallets, explorers, indexers, and analytics tools easier
to build.

Events must be specified with versioned schemas.

### 24.15 Versioning

Every HNS has its own version.

```text
HNS-1 v1
  -> HNS-1 v2
  -> HNS-1 v3
```

New versions must document compatibility and migration behavior.

Older versions may remain supported by wallets and explorers for long periods.

### 24.16 Standards Registry

HNChain should maintain an HNS registry.

Conceptual flow:

```text
Registry
  -> HNS-1
  -> Approved
```

Each standard should pass:

- public discussion
- technical review
- compatibility review
- security review
- specification publication
- conformance test publication

The registry improves discoverability. It must not prevent independent
experimentation.

Reference:

- `docs/standards/README.md`

### 24.17 Reference Implementations

Each important standard should provide:

- reference contract
- conformance tests
- usage examples
- security recommendations
- compatibility notes

Reference implementations are examples. The standard defines behavior.

### 24.18 Asset Security

Asset standards must address common risks:

- uncontrolled minting
- lost administrator authority
- unsafe pausing or freezing
- incorrect transfer logic
- incompatible version changes
- approval misuse
- metadata impersonation
- bridge representation confusion
- misleading stable asset claims

Every HNS must contain a Security Considerations section.

### 24.19 HN Asset Registry Research Direction

HNChain may support an ecosystem asset registry.

Conceptual labels:

```text
Verified
Audited
Official Metadata
Known Interfaces
```

The asset registry can help users distinguish known assets from unknown or
impersonating assets.

It must not prohibit permissionless asset creation.

Registry labels must be transparent and revocable when evidence changes.

### 24.20 Chapter Summary

HNChain standards should be:

- modular
- extensible
- backward-compatible
- practical to implement
- secure by default
- understandable to developers and users

Standards should serve as a shared language for the ecosystem, allowing
independent projects to interoperate without custom adapters.

## Chapter XXVI: Interoperability And Cross-Chain Architecture

### 25.1 Introduction

The modern blockchain ecosystem consists of many independent networks.

Each network may have its own:

- consensus rules
- account model
- token standards
- virtual machine
- economic model
- finality assumptions
- security assumptions

HNChain should not attempt to replace every other chain.

HNChain should be able to interact with other networks safely.

### 25.2 Goals

Cross-chain architecture should support:

- secure asset exchange
- message passing between networks
- smart contract interaction across networks
- verification of external blockchain state
- extensible integration architecture
- clear isolation from HNChain core consensus

### 25.3 High-Level Architecture

Conceptual architecture:

```text
HNChain
  -> Cross-Chain Interface
      -> Network A
      -> Network B
      -> Network C
```

HNChain core must remain independent from specific external networks.

External network differences should be handled through specified adapters.

### 25.4 Adapter Principle

HNChain should use adapter-based interoperability.

```text
HNChain Core
  -> Bridge API
  -> Bitcoin Adapter
  -> Ethereum Adapter
  -> Solana Adapter
  -> Future Network Adapters
```

The core does not need to understand every external chain directly.

Adapters encapsulate external chain rules, proof formats, finality assumptions,
and asset mapping.

### 25.5 Bridge API

HNChain should define a common Bridge API.

Candidate conceptual operations:

```text
connect()
verify()
transfer()
receive()
finalize()
```

These are not final method names.

Each adapter implements the interface according to the external network's
consensus, finality, proof, and asset rules.

### 25.6 Interaction Types

Interoperability should distinguish several scenarios:

- asset transfer
- message passing
- state verification
- remote execution, if accepted in the future

Each scenario has different security assumptions and should not be collapsed
into one generic "bridge" behavior.

### 25.7 External Data Verification

HNChain must not automatically trust external data.

Every cross-chain message must include evidence that can be verified according
to the relevant integration profile.

Verification mechanisms may include:

- light-client proofs
- finality proofs
- validator attestations
- threshold signatures
- optimistic challenge periods
- trusted committee models, if explicitly accepted

Each mechanism has different trust assumptions.

The trust model must be visible to users and applications.

### 25.8 Wrapped Assets

Wrapped assets may be represented using HNS-7.

Conceptual flow:

```text
Original Asset
  -> Verification
  -> Wrapped Asset
  -> HNChain
```

Withdrawal performs the reverse process.

HNS-7 defines representation and metadata. It does not define bridge security.

### 25.9 Cross-Chain Messages

Interoperability is not limited to tokens.

Conceptual message flow:

```text
Contract A
  -> Message
  -> Bridge
  -> Contract B
```

Cross-chain messaging can support applications that coordinate across networks.

The message standard must define ordering, replay protection, delivery failure,
finality assumptions, fee payment, and timeout behavior.

### 25.10 Security Principles

Bridge systems are frequent attack targets.

HNChain interoperability should follow:

- minimize trust
- verify messages
- limit bridge authority
- document trust assumptions
- isolate bridge failures
- require independent audits
- provide transparent monitoring

Security warning:

Bridges can become a larger risk than the base chain. No bridge should be
treated as safe merely because it is official or widely used.

### 25.11 Core Independence

Specific bridges should not be built into HNChain core consensus by default.

The core defines interfaces and validation boundaries.

Bridge implementations can evolve separately.

This reduces coupling and allows external integrations to be upgraded, audited,
deprecated, or removed without rewriting the base protocol.

### 25.12 Adding New Networks

The architecture should allow new integrations without changing the base
protocol.

```text
Bridge API
  -> New Adapter
  -> Compatible
```

Adding an adapter requires specification, test vectors, security review, and
clear trust assumptions.

### 25.13 Standard Compatibility

Developer tools may provide mappings between common external asset standards and
HNChain standards.

Examples:

- fungible token mapping
- NFT metadata mapping
- wrapped asset metadata
- event mapping
- address display mapping

Limitations and model differences must be documented. A token standard on one
chain rarely maps perfectly to another chain.

### 25.14 HN Bridge Registry

HNChain may maintain a bridge adapter registry.

Each adapter should publish:

- specification
- supported external networks
- version
- trust model
- proof model
- audit status
- compatibility tests
- known limitations

The registry helps users identify reviewed integrations.

It must not prevent independent bridge development.

### 25.15 Cross-Chain Events

External events may be normalized into HNChain event format.

```text
External Event
  -> Bridge
  -> HN Event
  -> Applications
```

Applications should not need custom parsing for every external network when a
standardized event mapping exists.

Event normalization must preserve source network, proof, finality, and trust
metadata.

### 25.16 Failure Isolation

External chain failure must not compromise HNChain core operation.

If an external blockchain or bridge adapter is unavailable:

- HNChain consensus must continue
- unrelated transactions must continue
- bridge operations may pause or fail safely
- wrapped asset operations may enter a defined safe state
- users and applications must receive clear status

Cross-chain components must be isolated from core liveness and finality.

### 25.17 Interoperability Profiles

HNChain should define interoperability profiles.

Examples:

```text
HNIP-Bridge
HNIP-Message
HNIP-Assets
HNIP-Proof
```

These profiles allow bridge interfaces, message formats, asset mapping, and
proof verification to evolve independently.

Reference:

- `docs/rfc/interoperability/README.md`

### 25.18 Chapter Summary

HNChain interoperability should:

- be modular
- remain independent from specific external networks
- use shared interfaces
- support secure message and asset exchange
- document trust assumptions
- isolate failures
- evolve through open specifications

## Chapter XXVII: Performance And Scalability

### 26.1 Performance As Engineering

HNChain treats performance as an engineering result, not a marketing label.

Performance must be:

- measurable
- reproducible
- scalable
- documented
- comparable across versions

Every published performance value must include the conditions under which it was
measured.

### 26.2 No Headline Numbers Without Methodology

If HNChain publishes a value such as:

```text
100,000 TPS
```

the result must be accompanied by:

- hardware profile
- number of validators
- number of full nodes
- transaction size
- transaction type mix
- network latency
- block size
- state size
- storage backend
- VM workload
- test duration
- measurement methodology

Without this context, TPS is not an engineering result.

### 26.3 Current Targets

At the current design stage, HNChain defines targets, not guaranteed
production characteristics.

Current target direction:

```text
Block Time     -> approximately 500-800 ms
Finality       -> <= 2 seconds
Confirmation   -> approximately 1-2 blocks
Target TPS     -> determined by benchmark results
```

Earlier throughput targets are ambition markers. They must not be treated as
achieved until validated by open and reproducible benchmarks.

### 26.4 What Affects TPS

Throughput depends on the whole system.

```text
Network
  -> Consensus
  -> Storage
  -> HNVM
  -> CPU
  -> Memory
```

If one subsystem becomes the bottleneck, total throughput decreases.

TPS cannot be designed by consensus alone.

### 26.5 Parallel Execution

Parallel execution is one of HNChain's key research directions.

If two transactions do not interact with the same state, they may be executed in
parallel.

Example:

```text
TX A -> Wallet A
TX B -> Wallet Z
```

If there is no read/write or write/write conflict, the transactions may be
parallelizable.

If conflicts exist, execution order must be determined by protocol rules.

### 26.6 Parallel Scheduler

Parallel execution requires a deterministic scheduler.

```text
Transactions
  -> Dependency Analysis
  -> Parallel Scheduler
  -> HNVM
```

The scheduler must preserve deterministic state roots.

It must define:

- dependency analysis
- access graph validation
- conflict detection
- deterministic ordering
- rollback behavior
- failure handling
- resource accounting

Parallelism is allowed only when it does not change consensus results.

### 26.7 Memory Efficiency

Performance is not only CPU speed.

HNVM and node implementations should aim to:

- minimize unnecessary data copying
- use local buffers where safe
- release temporary memory efficiently
- avoid unnecessary allocations
- bound memory use under adversarial input

Concrete techniques depend on implementation language and runtime.

Memory optimization must not introduce nondeterministic execution behavior.

### 26.8 Block Propagation

Fast block propagation is critical for low finality latency.

Conceptual flow:

```text
Leader
  -> Header
  -> Peers
  -> Body
  -> Finalized
```

Some validation and preparation may happen before the full block body arrives.

The network specification must ensure that header-first propagation cannot be
used to trigger unbounded expensive work.

### 26.9 Storage Optimization

Storage performance affects block verification and node responsiveness.

Conceptual flow:

```text
Block
  -> Write
  -> Index
  -> Ready
```

Storage optimization should include:

- atomic writes
- efficient indexes
- batched updates
- snapshot reads
- predictable compaction behavior
- corruption detection
- bounded write amplification

Storage optimizations must not change canonical state or proof semantics.

### 26.10 Cache System

HNChain implementations may use layered caching.

Conceptual model:

```text
L1
  -> L2
  -> Persistent
```

Cache categories:

- hot state
- recently used objects
- decoded blocks
- transaction validation results
- VM execution artifacts

Caches are local optimizations.

Cache contents must not affect deterministic execution, state roots, or
consensus validity.

### 26.11 Scaling Dimensions

HNChain can scale in multiple ways:

Vertical scaling:

- more capable hardware
- faster storage
- more memory
- better networking

Horizontal scaling:

- more independent nodes
- better peer topology
- broader validator participation
- improved propagation

Architectural scaling:

- better algorithms
- parallel execution
- improved state layout
- better proof systems
- protocol upgrades without application rewrites

Scaling must not be achieved by silently sacrificing decentralization.

### 26.12 Performance Profiles

HNChain should define standard benchmark profiles.

Examples:

```text
Small Network
  -> 10 nodes

Medium Network
  -> 100 nodes

Large Network
  -> 1000+ nodes
```

Profiles allow performance comparisons across versions and client
implementations.

Profile definitions must include hardware, latency, topology, workload, and
duration.

### 26.13 Benchmark Suite

Every major HNChain version should run the same benchmark suite.

Subsystems:

```text
Consensus
  -> Storage
  -> Network
  -> HNVM
  -> RPC
```

If performance regresses, the regression should be visible and explainable.

Benchmarks should include normal operation, stress tests, and adversarial input
where safe.

### 26.14 Metrics

HNChain should publish more than TPS.

Required metrics should include:

- finality time
- block propagation delay
- memory usage
- CPU usage
- network traffic
- new-node synchronization time
- contract execution speed
- RPC latency
- storage read latency
- storage write latency
- failure recovery time

A complete performance report should show throughput, latency, resource usage,
and reliability together.

### 26.15 Performance Dashboard

HNChain may provide an official performance dashboard.

Candidate views:

```text
TPS
CPU
RAM
Latency
Peers
Finality
Storage
RPC
```

The dashboard can help node operators and developers observe network behavior.

Dashboard data is operational telemetry. It does not define consensus truth.

### 26.16 Scalability Without Accidental Complexity

Optimization must not make the protocol incomprehensible.

If a small performance gain sharply increases implementation complexity,
consensus risk, or audit cost, the trade-off must be reviewed carefully.

Performance work must preserve:

- determinism
- safety
- testability
- compatibility
- independent implementation feasibility

### 26.17 HN Performance Lab

HNChain should maintain an official performance lab and load-test scenarios.

Conceptual scenarios:

```text
10 TPS
  -> 100 TPS
  -> 1,000 TPS
  -> 10,000 TPS
  -> Stress Test
```

Each scenario must define input parameters, expected measurements, and reporting
format.

This allows client implementations to be compared under identical conditions.

Reference:

- `docs/performance/README.md`

### 26.18 Chapter Summary

HNChain performance must be:

- measurable
- reproducible
- documented
- scalable
- based on objective tests
- reported with experiment conditions

Performance claims without methodology are not accepted.

## Chapter XXVIII: HN Ecosystem Architecture

### 27.1 Introduction

HNChain is not only a blockchain protocol.

For developers, users, and operators to work effectively, HNChain needs a set of
interoperable tools built around shared specifications.

The ecosystem should support:

- application development
- smart contract deployment
- digital asset management
- node operation
- network analysis
- external service integration

The guiding principle is:

```text
The protocol should be minimal. The ecosystem should be rich.
```

Complexity should live in tools, libraries, and services where possible, not in
base network rules.

### 27.2 High-Level Ecosystem Architecture

Conceptual ecosystem architecture:

```text
HNChain
  -> Developer
      -> HN SDK
      -> HN CLI
      -> HN IDE
      -> HN Package Registry
  -> Infrastructure
      -> Validator Toolkit
      -> Explorer
      -> Scan API
      -> Metrics
  -> End Users
      -> Wallet
      -> Identity
      -> HN DNS
```

All components should interact through open APIs and shared standards.

### 27.3 HN Wallet

HN Wallet should be an official reference user client.

Core capabilities:

- HNC management
- HNS asset support
- multi-account wallet
- hardware wallet integration
- multisignature
- transaction history
- smart contract interaction
- staking and validator workflows, if supported

The wallet architecture should be modular so new features can be added without
rewriting base wallet logic.

Official wallet status must not prevent independent wallet implementations.

### 27.4 HN Explorer

HN Explorer provides transparent network visibility.

It should display:

- blocks
- transactions
- validators
- contracts
- tokens
- NFTs
- DAO activity
- network statistics

Explorer must use open interfaces available to third-party developers.

Explorer views are indexed representations, not consensus truth.

### 27.5 HN SDK

Official SDKs should provide a consistent developer experience across languages.

Planned SDKs:

- Rust
- Go
- TypeScript
- Python
- Java
- Kotlin
- Swift
- C#

SDKs should share concepts, naming, error models, and compatibility tests where
language conventions allow.

### 27.6 HN CLI

HN CLI should combine common developer and operator tasks.

Candidate commands:

```text
hn init
hn build
hn test
hn deploy
hn wallet
hn validator
hn node
hn rpc
```

The CLI is a unified ecosystem interface.

It must call documented APIs and must not contain hidden protocol logic.

### 27.7 HN IDE

HNChain should support integrations for popular development environments.

Features may include:

- syntax highlighting
- autocompletion
- local compilation
- test execution
- contract debugging
- static analysis
- deployment helpers

This should be implemented as editor and IDE extensions, not as a requirement to
use one official IDE.

### 27.8 HN Package Registry

HNChain may provide a package registry for reusable components.

Conceptual structure:

```text
Registry
  -> Smart Contracts
  -> Libraries
  -> Templates
  -> Utilities
```

Each package should include:

- version
- description
- license
- checksum
- changelog
- source provenance
- compatibility metadata

Package registry security requires signing, reproducible builds, dependency
review, and vulnerability disclosure.

### 27.9 HN Developer Portal

HN Developer Portal should be a unified entry point.

It should include:

- documentation
- HNIP index
- code examples
- SDKs
- API references
- guides
- security recommendations
- learning materials
- compatibility tests

The portal improves onboarding, but the protocol must remain understandable from
open specifications alone.

### 27.10 HN Faucet

HN Faucet provides testnet coins for developers.

The faucet is a testnet service only.

It must include rate limits, abuse controls, and clear separation from mainnet
funds.

### 27.11 HN Testnet

HN Testnet is used for:

- upgrade testing
- contract testing
- developer training
- load experiments
- wallet and explorer integration
- validator operation practice

Testnet behavior may differ from mainnet, but differences must be documented.

### 27.12 HN Devnet

HN Devnet supports early experimentation.

It may change faster than Testnet.

Devnet is appropriate for unstable features, early prototypes, and pre-review
testing.

Devnet must not be treated as a production compatibility signal.

### 27.13 HN Validator Toolkit

HN Validator Toolkit should help operators run nodes safely.

Capabilities may include:

- installation
- upgrades
- backups
- monitoring
- diagnostics
- metrics export
- key separation guidance
- configuration validation

Operator tooling reduces operational mistakes, but consensus correctness still
depends on protocol implementation and validator behavior.

### 27.14 HN Scan API

HN Scan API provides indexed blockchain data.

Supported query areas may include:

- blocks
- accounts
- assets
- contracts
- events
- network statistics

HN Scan API must follow the general API versioning policy.

Indexed data must expose source references so clients can verify important
values against node APIs or proofs.

### 27.15 HN Identity

HN Identity is an optional digital identity component.

It may support:

- voluntary identity verification
- document signing
- reputation systems
- DAO integration

The base protocol must not require mandatory user identification.

HN Identity must preserve privacy, consent, revocation, and data minimization.

### 27.16 HN DNS

HN DNS is a possible readable-name system.

Examples:

```text
alice.hn -> Address
market.hn -> Smart Contract
```

Readable names improve UX and reduce address-entry errors.

Name resolution must define ownership, expiration, recovery, disputes,
anti-phishing protections, and fallback behavior.

### 27.17 HN Ecosystem Registry

HNChain may maintain a registry of ecosystem components.

Each entry may include:

- name
- version
- maintainer
- license
- documentation
- compatibility
- security status

The registry helps users discover compatible tools.

It must not become a gatekeeper that prevents independent development.

### 27.18 Independent Implementations

HNChain may provide official tools, but the ecosystem remains open.

Independent developers may create:

- wallets
- explorers
- SDKs
- node clients
- IDE plugins
- package libraries
- monitoring systems

Compatibility comes from open specifications and tests, not internal
dependencies on official tools.

### 27.19 Compatibility Principle

Ecosystem components should interoperate through open standards.

They should avoid private APIs, hidden assumptions, and undocumented coupling.

This reduces incompatible islands inside the platform.

### 27.20 HN Certification Research Direction

HNChain may define a voluntary compatibility certification program.

Conceptual status flow:

```text
Compatible
  -> Tested
  -> Certified
```

Certification criteria must be public, reproducible, and based on compatibility
tests.

Certification must not become permission to participate in the ecosystem.

Reference:

- `docs/ecosystem/README.md`

### 27.21 Chapter Summary

HNChain ecosystem should:

- be built around open specifications
- provide tools for development and operations
- support independent implementations
- preserve compatibility between components
- reduce onboarding friction
- avoid expanding the protocol core unnecessarily

## Chapter XXIX: Roadmap, Release Strategy And Long-Term Vision

### 28.1 Introduction

HNChain is designed as a long-term technology platform.

The project should evolve gradually through open specifications, testing,
security review, and independent implementations.

The purpose of the roadmap is not to predict a fixed feature calendar.

The purpose is to define an evolution strategy and the criteria required before
moving between stages.

### 28.2 Development Principles

Every change should satisfy:

- necessity
- verifiability
- backward compatibility where possible
- security
- transparency
- documentation

Features should not be added only because they are popular in other projects.

### 28.3 Development Phases

HNChain development should follow staged phases:

```text
Research
  -> Prototype
  -> Developer Preview
  -> Devnet
  -> Testnet
  -> Mainnet
  -> Long-Term Support
```

Each phase must have exit criteria.

### 28.4 Phase 0: Research

Research focuses on:

- specifications
- architecture modeling
- threat analysis
- HNVM design
- consensus analysis
- cryptographic profile selection
- storage and state tree design
- economic modeling

At this stage, design quality is more important than implementation speed.

### 28.5 Phase 1: Prototype

Prototype creates the first working implementation.

Primary goal:

```text
Validate architecture viability.
```

Protocol structure may still change.

Compatibility between prototype versions is not guaranteed.

Prototype code must be clearly labeled as non-production.

### 28.6 Phase 2: Developer Preview

Developer Preview introduces early tools.

Candidate components:

- HN CLI
- HN SDK
- HN Wallet
- HN Explorer
- local development network
- sandbox execution
- basic documentation

This phase is for developer feedback and workflow validation.

### 28.7 Phase 3: Devnet

Devnet is an experimental network.

Goals:

- consensus testing
- performance testing
- HNVM testing
- contract execution testing
- P2P testing
- architectural error discovery

Devnet may reset or change frequently.

Devnet stability must not be interpreted as mainnet readiness.

### 28.8 Phase 4: Testnet

Testnet is a public testing network.

Goals:

- stability testing
- security audit support
- application testing
- validator operation testing
- load testing
- upgrade testing
- compatibility testing

Critical changes must pass through Testnet before Mainnet activation.

### 28.9 Phase 5: Mainnet

Mainnet launch is allowed only when predefined readiness criteria are satisfied.

Example criteria:

- independent audits completed
- critical issues resolved
- protocol specifications published
- core tooling available
- compatibility tests available
- upgrade process tested
- incident response process defined
- node operation documentation published
- security disclosure process active

Mainnet readiness must be based on evidence, not schedule pressure.

### 28.10 Long-Term Support

After protocol stabilization, HNChain may publish LTS releases.

```text
HNChain 1.x
  -> Security Updates
  -> Bug Fixes
  -> LTS
```

LTS is important for infrastructure operators, enterprises, wallets, exchanges,
and archival services.

LTS policy must define support windows, security update scope, compatibility,
and deprecation timelines.

### 28.11 Release Strategy

Each release should follow a predictable process:

```text
Specification
  -> Implementation
  -> Testing
  -> Audit
  -> Release Candidate
  -> Production
```

Skipping specification, testing, or review for production releases is not
acceptable for consensus-critical components.

### 28.12 Semantic Versioning

HNChain should use SemVer-style versioning:

```text
Major.Minor.Patch

1.0.0
  -> 1.1.0
  -> 1.1.1
  -> 2.0.0
```

Meaning:

- Major: incompatible protocol changes
- Minor: backward-compatible functionality
- Patch: compatible bug fixes

Consensus versioning may require additional activation metadata such as height,
epoch, network, or governance decision identifier.

### 28.13 Upgrade Policy

Every release must include:

- change description
- upgrade instructions
- compatibility analysis
- known limitations
- migration steps, if needed
- rollback or recovery notes, where applicable
- operator impact
- security considerations

This reduces upgrade risk for validators, node operators, wallets, exchanges,
and applications.

### 28.14 Deprecation Policy

Feature removal should be gradual.

```text
Supported
  -> Deprecated
  -> Removal
```

Users and operators must receive clear notice and enough time to migrate.

Deprecation must define:

- affected feature
- replacement path
- support window
- removal conditions
- compatibility impact
- security implications

### 28.15 Pre-Implementation Requirements

HNChain should not begin production implementation until the following are
accepted:

- ADR-0000 Protocol Invariants
- ADR-0001 Account Model
- ADR-0002 Cryptographic Identity
- ADR-0003 Address Format
- ADR-0004 Canonical Serialization
- ADR-0005 Hash Algorithms
- ADR-0006 Transaction Format
- ADR-0007 State Tree
- ADR-0008 Block Format
- initial genesis manifest and document commitment model
- initial consensus safety model
- initial storage model
- initial snapshot and archival model
- initial HNVM execution model
- initial network packet model
- initial P2P capability negotiation model
- initial wallet key management and signing model
- initial account permission and wallet API model
- initial RPC, API, SDK, and CLI model
- initial governance and HNIP process
- initial compatibility test suite model
- initial threat model and incident response process
- initial HNS standards and asset registry model
- initial interoperability and bridge adapter model
- initial benchmark methodology and performance profiles
- initial ecosystem registry and certification model

Prototype code may be written only when explicitly labeled non-production and
kept separate from protocol commitments.

### 28.16 Success Metrics

HNChain should not measure success only by transaction count or token market
price.

Healthier maturity metrics include:

- number of independent client implementations
- number of ecosystem developers
- community activity
- audit results
- network stability
- compatibility between versions
- documentation quality
- test coverage
- reproducible builds
- time to recover from incidents
- number of independently operated nodes

These metrics better reflect platform maturity.

### 28.17 Sustainability

Long-term development requires process maturity, not only technical ambition.

HNChain should maintain:

- open documentation
- transparent development
- reproducible builds
- independent verification
- regular security updates
- compatibility tests
- public release notes
- clear governance procedures

### 28.18 Long-Term Vision

HNChain's long-term goal is to become a durable platform for distributed
applications that does not depend on one company or one team.

Project success is not defined by the number of features implemented.

Success is defined by whether the system remains reliable, understandable,
secure, compatible, and maintainable over time.

Guiding principles:

```text
Open Specifications
  -> Independent Implementations
  -> Security First
  -> Compatibility
  -> Transparency
  -> Long-Term Evolution
```

### 28.19 Whitepaper Conclusion

This concludes the main architectural part of the HNChain Whitepaper.

HNChain is described as:

- an open blockchain protocol
- a modular virtual machine
- an extensible standards system
- an ecosystem of tools
- a platform for long-term evolution

Technical details must evolve through open HNIP, ADR, RFC, and specification
processes, supported by tests and independent implementations.

Reference:

- `docs/roadmap/README.md`

## Documentation System

HNChain uses four primary document families.

The full documentation tree is defined in:

- `docs/README.md`

### Whitepaper

Purpose:

- mission
- philosophy
- economic direction
- consensus direction
- long-term design intent

The whitepaper is strategic. It is not sufficient for implementation.

### RFC

Purpose:

- exact protocol specifications
- binary formats
- network packets
- RPC methods
- state transition rules
- test vectors

RFCs are implementation-facing.

### HNVM Specification

Purpose:

- virtual machine semantics
- contract execution model
- metering
- ABI
- host functions
- deterministic behavior
- security constraints

### HNIP

Purpose:

- improvement proposals
- governance-visible protocol changes
- backward compatibility analysis
- migration plans
- ecosystem coordination

HNIP is the long-term change management process.

### Developer Docs

Purpose:

- application development guides
- SDK usage
- wallet integration
- RPC usage
- smart contract examples
- local node operation

Developer docs explain how to use HNChain. They do not define consensus truth.

## Glossary

Account:

- A versioned protocol state object identified by an address and bound to
  cryptographic identity through key descriptors, permissions, or identity
  commitments.

ADR:

- Architecture Decision Record. A document that records a major architectural
  decision, its rationale, alternatives, consequences, and risks.

Bridge:

- A cross-chain component that verifies or transports assets, messages, or
  proofs between HNChain and another network.

Consensus:

- The mechanism by which independent nodes agree on valid block history and
  resulting state.

Finality:

- The condition under which a block is considered irreversible under stated
  protocol security assumptions.

HNC:

- The native coin of HNChain.

HNCS:

- HNChain Canonical Serialization. The canonical binary encoding profile for
  consensus objects.

HNIP:

- HN Improvement Proposal. The proposal process for protocol and ecosystem
  changes.

HNVM:

- HN Virtual Machine. The deterministic execution environment for smart
  contracts.

HNS:

- HN Standards. Ecosystem standards for assets, interfaces, metadata, events,
  and compatibility.

Mempool:

- The local node component that stores and propagates transactions before they
  are included in finalized blocks.

RPC:

- Remote Procedure Call interface exposed by nodes or gateways for applications,
  wallets, explorers, SDKs, and tooling.

Snapshot:

- A verified representation of chain state at a specific height or checkpoint,
  used for synchronization and recovery.

State Root:

- A cryptographic commitment to the canonical blockchain state.

Validator:

- A network participant authorized by protocol rules to propose, validate, or
  vote on blocks.

## References

Primary HNChain documents:

- `docs/README.md`
- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0001-account-state-model.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0003-address-format.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/specs/core/account-state.md`
- `docs/specs/core/cryptographic-identity.md`
- `docs/specs/core/address-format.md`
- `docs/specs/core/canonical-serialization.md`
- `docs/specs/core/hash-algorithms.md`
- `docs/specs/core/genesis.md`
- `docs/specs/core/transaction-format.md`
- `docs/specs/core/state-tree.md`
- `docs/specs/core/block-format.md`
- `docs/rfc/consensus/consensus-architecture.md`
- `docs/governance/constitution.md`
- `docs/security/README.md`
- `docs/standards/README.md`
- `docs/performance/README.md`
- `docs/ecosystem/README.md`
- `docs/roadmap/README.md`

External references:

- Bitcoin: A Peer-to-Peer Electronic Cash System
  https://bitcoin.org/bitcoin.pdf
- RFC 8032: Edwards-Curve Digital Signature Algorithm
  https://datatracker.ietf.org/doc/html/rfc8032
- RFC 8410: Algorithm Identifiers for Ed25519, Ed448, X25519, and X448
  https://datatracker.ietf.org/doc/html/rfc8410
- RFC 8949: Concise Binary Object Representation
  https://www.rfc-editor.org/info/rfc8949/
- BIP 173: Base32 address format for native v0-16 witness outputs
  https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki
- BIP 350: Bech32m format
  https://github.com/bitcoin/bips/blob/master/bip-0350.mediawiki
- NIST FIPS 180-4: Secure Hash Standard
  https://csrc.nist.gov/pubs/fips/180-4/upd1/final
- NIST FIPS 202: SHA-3 Standard
  https://csrc.nist.gov/pubs/fips/202/final
- NIST FIPS 204: Module-Lattice-Based Digital Signature Standard
  https://csrc.nist.gov/pubs/fips/204/final
- SEC 2: Recommended Elliptic Curve Domain Parameters
  https://www.secg.org/sec2-v2.pdf

## Non-Goals

HNChain does not aim to:

- maximize benchmark numbers at the expense of security
- depend on one official implementation forever
- hide protocol behavior in code
- use undocumented binary formats
- accept nondeterministic smart contract execution
- require users to understand low-level cryptography for routine use
- make irreversible architectural decisions without documented trade-offs
- become a game engine
- become a social network
- become a general-purpose file storage system
- become an operating system
- exist primarily as a meme asset

HNChain is a base protocol. Applications, games, social systems, storage
services, and other products may be built on top of it, but they are not the
core protocol itself.

## Current Status

HNChain is in the architecture and specification phase.

Accepted:

- protocol invariants
- extended account-based state model

Proposed:

- cryptographic identity
- address format
- canonical serialization
- hash algorithms
- transaction format
- state tree
- block format
- consensus architecture

Not yet specified:

- final consensus protocol
- storage layout
- P2P protocol
- RPC API
- HNVM
- economics
- governance
