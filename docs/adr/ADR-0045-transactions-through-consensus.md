# ADR-0045: Transactions Through Consensus

Status: Accepted

Date: 2026-10-12

Version: 0.1.0

Depends On:

- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0008: Block Format
- ADR-0030: Transaction And Block Application
- ADR-0031: Write-Set Values And Overlay State Reader
- ADR-0033: Atomic Write-Set Commit
- ADR-0037: Multi-Node Consensus Wiring
- ADR-0042: BlockEnvelope And Receiver-Side Header Verification
- ADR-0043: Genesis As A Real Block 0
- ADR-0044: Full-Verification Block And State Sync (cited here as
  motivation — its own implementation is currently untestable against
  only-ever-empty blocks; it does not depend on this ADR's own types
  or decisions the other way around)

Supersedes: None (narrows ADR-0037's own "blocks are always empty"
scope, and replaces the fixed-devnet-value `state_root` check ADR-0040/
ADR-0042 established with real execution — see "Decided: Proposal
Validation Executes The Block," below)

Referenced By: None

## Context

Every block `hn-node` produces today has `body.transactions == []` —
`Vec::new()` at every `BlockBody` construction site in `node.rs`.
Consequences, all named in earlier ADRs but never closed:

- There is no mempool and no path by which a transaction can reach a
  proposer (ADR-0006 lists "mempool admission policy" as open).
- `state_root`/`consensus_root`/`protocol_parameters_hash` are checked
  by receivers against fixed devnet values (ADR-0042), so the check
  proves nothing about real state.
- ADR-0044 (full-verification sync) cannot be implemented or even
  tested: replaying empty blocks proves nothing.
- `apply_block` explicitly does not compute `state_root` ("needs the
  complete current leaf set, which nothing in this project maintains
  yet").

What already exists and is reused unchanged: `TransactionEnvelope`,
`apply_transaction`/`apply_block`/`apply_and_commit_block`,
`OverlayReader`, `compute_state_root(entries, empty_table)`,
`ConsensusEngine::commit_finalized_block` (already applies the cached
proposal's transactions via `apply_and_commit_block`), `Channel::
Transactions` + `MessageType::TransactionAnnounce` in `hn-network`
(reserved, never wired), `MAX_TRANSACTIONS_PER_BLOCK` (10,000,
`hn-state`), `MAX_TRANSACTION_SIZE` (256 KiB, `hn-state`), and
`MAX_PAYLOAD_LEN` (8 MiB, `hn-network`'s own P2P envelope cap — the
closest existing analog to a "max block size" bound; no dedicated
`MAX_BLOCK_SIZE` constant actually exists anywhere in this codebase
today, despite one being informally referenced in `envelope.rs`'s own
doc comment).

## Decision

### Decided: Mempool Is Local Policy, Not Consensus

Consistent with ADR-0009 and ADR-0011 ("mempool order as consensus" is
rejected): mempool contents, ordering, and eviction are per-node local
policy. Block validity never depends on any node's mempool.

### Decided: Mempool Admission

A transaction is admitted only if, in order:

1. Canonical decode succeeds and encoded size `<= MAX_TRANSACTION_SIZE`
   (size check before decode, ADR-0006's own pipeline order).
2. `chain_id`/`network_id` match this node.
3. Signature verification passes.
4. The validity window (height-based, ADR-0006) contains
   `current_height + 1`.
5. A dry run via `apply_block(&[tx], committed_state, ...)` succeeds —
   exactly the "simulate without persisting" use case `apply_block`'s
   own doc comment already reserves for mempool validation.
6. `tx_id` is not already present.

Capacity is a local, non-consensus constant (`MEMPOOL_MAX_TXS`, a
devnet value chosen in implementation, documented as non-protocol).
Eviction: reject new transactions when full (simplest policy — no fee
market exists yet to prioritize by). Entries are removed when included
in a finalized block, or when their validity window expires.

### Decided: Transaction Gossip

An admitted transaction is announced once via `TransactionAnnounce` on
`Channel::Transactions`. A received announce goes through the same
admission pipeline; duplicates (by `tx_id`) are dropped silently and
not re-broadcast. Admission failure is a dropped, logged message,
never a fatal error (ADR-0042's own no-crash principle for network
input, applied here too).

### Decided: Devnet Transaction Entry Point

No RPC exists (`hn-rpc` is a stub), so a devnet-only entry point is
needed for tests: a minimal client that performs the existing `Hello`
handshake with a node and sends one `TransactionAnnounce`. It is a test
utility under `hn-node/tests/support`, not a public API, and does not
pre-empt `hn-rpc`.

### Decided: Block Building (Proposer)

The proposer builds the body from its own mempool:

1. Select candidates: per sender, ascending by nonce; across senders,
   mempool arrival order. (The order is the proposer's own choice —
   validators verify the *result*, not the selection.)
2. Stop at `MAX_TRANSACTIONS_PER_BLOCK`, or when the encoded body
   would exceed the practical wire-size ceiling (`MAX_PAYLOAD_LEN`
   today — see "Context," above; a dedicated block-size bound may be
   introduced during implementation).
3. Run `apply_block` over the selection against committed state at
   `height - 1`. A transaction that makes `apply_block` return an
   error is removed from the selection and the run is repeated — a
   proposal must never contain a transaction that aborts block
   application.
4. Fill `body.transactions`, `body.receipts`, header
   `transactions_root`/`receipts_root` from the result, and
   `state_root` from the procedure decided below.

An empty selection yields an empty block, exactly as today — empty
blocks remain valid.

### Decided: Failed-At-Execution Transactions Are Excluded, Not Included With A `Failed` Receipt

A transaction that fails `apply_block`'s own dry run (step 3, above)
is removed from the proposer's selection entirely — it does not appear
in `body.transactions`, consumes no nonce, and produces no `Failed`
receipt. This is the accepted, decided behavior, not a devnet-only
placeholder pending revisit: ADR-0006's own "nonce consumed on
inclusion" principle is satisfied exactly because exclusion means
never being included in the first place, not a weaker reading of it.

This does not foreclose a *future* fee model introducing its own,
separate reason to charge for (and therefore include) a failed
execution as an anti-spam measure — see "Open Decisions," below — but
that is a fee-model question for its own future ADR, not an open fork
in this one. Excluding is this ADR's own final answer for how
`hn-node` builds and validates blocks today.

### Decided: Proposal Validation Executes The Block

This is the key change, and the user's own explicit call: a validator
does not trust the proposer's claimed `state_root`. After every
ADR-0042 check passes, a validator additionally:

1. Runs `apply_block(body.transactions, committed_state, height, ...)`.
   Any error means the proposal is invalid.
2. Requires the recomputed `transactions_root`/`receipts_root` to equal
   the header's own claimed values, and the recomputed receipts to
   equal `body.receipts`.
3. Requires the recomputed post-state `state_root` to equal
   `header.state_root`.

Failure at any step: drop the proposal, log it, and do not prevote for
it (prevote nil via the existing timeout path) — never fatal, the same
"externally-triggerable input must not crash this process" principle
ADR-0042 already established. **ADR-0042's own check against a fixed
devnet `state_root` is removed; this ADR supersedes it** for
`state_root` specifically — `consensus_root`/`protocol_parameters_hash`
remain fixed-devnet-value checks, unchanged (ADR-0010's own still-open
item).

### Decided: `state_root` Computation Strategy (Devnet)

`compute_state_root` requires the complete leaf set. The state store
gains an enumeration method (a `StateReader` extension, or a separate
`StateEnumerator` trait — exact shape left to implementation, behavior
specified here): return every `(state_key, value)` pair in the
committed state. The post-state root is `compute_state_root` over the
committed leaves with the block's own write-set overlaid.

This is `O(N)` in total state size, per block, per validator. Accepted
explicitly for devnet, where `N` is tiny, and named as non-scalable:
replacing it with an incrementally maintained, persisted sparse Merkle
structure (`hn-storage`) is a separate future ADR, and a hard
prerequisite for testnet-scale state.

Block 0 (ADR-0043) keeps its existing genesis `state_root`; the first
real block's pre-state is the genesis state.

### Decided: Commit

Unchanged: on `Finalized`, `ConsensusEngine::commit_finalized_block`
applies and commits the cached proposal's transactions atomically
(ADR-0033). After commit, included transactions are removed from the
mempool, and the remaining ones are re-validated lazily on the next
selection.

## Rejected Options

### Trust The Proposer's `state_root` Until QC

Rejected. A validator that prevotes without executing can be made to
prevote for a block with an arbitrary `state_root`; `2f+1` such votes
would finalize a header that does not match the state any honest node
actually derives, and ADR-0044's own replay would then fail on every
syncing node. This is the same class of hole ADR-0042 closed for
`block_hash`.

### Incremental SMT Now

Rejected for this ADR. It is the right long-term answer, but is a
storage design (persisted internal nodes, pruning interaction, ADR-0007
conformance) larger than the gap being closed here. Full recompute is
correct, simple, and provably equal by construction; an optimization
can be validated against it later as an oracle.

### Mempool Order As Consensus

Rejected (ADR-0009): arrival order differs across nodes.

### Fee Market / Fee Priority Now

Rejected. Fee deduction is still not applied in `apply_transaction`
(ADR-0030); fee semantics belong to the economics specification, not
to this ADR (the project's own Economic Decision Constraint).

### Including Failed-At-Execution Transactions With A `Failed` Receipt

Rejected, as the final decision for this ADR (see "Decided:
Failed-At-Execution Transactions Are Excluded," above) — not merely
deferred. Including them would charge no real fee today (none exists,
ADR-0030) while still consuming a nonce, for no real benefit over
simply excluding; the anti-spam case for inclusion only exists once a
real fee model exists to actually charge for the attempt, which is a
distinct, future decision this ADR does not need to make now.

## Security Considerations

- Admission signature verification bounds the cost an unauthenticated
  peer can impose; the mempool cap bounds memory. Real DoS/rate
  limiting of the `Transactions` channel remains open, with the rest
  of P2P hardening.
- A malicious proposer can include transactions that are individually
  valid but censor others; censorship resistance is a liveness/
  fairness property, not addressed here.
- Validator execution cost per proposal is bounded by
  `MAX_TRANSACTIONS_PER_BLOCK` and the practical wire-size ceiling; a
  round timeout must be long enough to execute a full block — to be
  checked against devnet timeouts in implementation (devnet tests use
  150-200 ms rounds with empty blocks today).
- Execution makes `apply_block` panics/overflows consensus-critical:
  implementation must keep the existing no-panic discipline and treat
  any internal error during validation as "proposal invalid," never a
  process exit.

## Compatibility

Changes what a conforming receiver accepts (it now executes the block
and checks the recomputed `state_root`, not a fixed devnet value).
Pre-mainnet and devnet-only, so no migration is needed; existing tests
that assert a fixed devnet `state_root` are updated as part of
implementation.

## Conformance / Test Requirements

- Unit: mempool admission (each rejection reason), dedup, capacity,
  eviction on inclusion/expiry.
- Unit: block builder respects count/size limits, drops a transaction
  that aborts `apply_block`, orders per-sender by nonce.
- Unit: proposal validation rejects a wrong `state_root`, wrong
  `receipts_root`, wrong receipts, and a transaction that aborts
  application; accepts both empty and non-empty correct blocks.
- Unit: `state_root` recomputation equals an independent Python-oracle
  value for a fixed genesis plus one transfer vector (a new vector,
  oracle-verified, matching this project's own standing discipline for
  every crypto/encoding value).
- Integration (4 real processes): inject a transfer into node A; all
  four nodes finalize a block with `applied=1`, matching balances and
  `state_root` across nodes; restart one node and verify it converges
  (this also exercises the still-open passive-catchup gap ADR-0044 is
  meant to eventually replace, so the expected result is documented
  explicitly rather than assumed).

## Open Decisions

- fee deduction and proposer/validator fee distribution (a future
  economics specification)
- incremental, persisted SMT replacing the `O(N)` full recompute above
  (a separate future ADR, a hard prerequisite for testnet-scale state)
- peer-level DoS/rate limiting on `Channel::Transactions`
- `validator_candidates`' own source for transaction application
  inside the node (currently `ctx.records` — confirm this stays
  correct as committed state evolves via stake/validator-update
  transactions)
- real `consensus_root`/`protocol_parameters_hash` (ADR-0010,
  unchanged — this ADR's own validation only replaces the `state_root`
  check, not those two)
- whether a future fee model gives failed-at-execution transactions a
  distinct, charged, included treatment as an anti-spam measure (not a
  reopening of "Decided: Failed-At-Execution Transactions Are
  Excluded," above — that is settled; this is a narrower question for
  whatever ADR eventually specifies fees)

## Related Specifications

- `docs/specs/core/transaction-format.md`
- `docs/specs/core/block-format.md`
- `docs/specs/core/state-tree.md`
