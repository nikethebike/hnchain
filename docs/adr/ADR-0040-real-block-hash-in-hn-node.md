# ADR-0040: Real Block Hash In `hn-node`

Status: Accepted

Date: 2026-09-25

Version: 0.1.0

Depends On:

- ADR-0007: State Tree
- ADR-0008: Block Format
- ADR-0010: Validator Set Model
- ADR-0037: Multi-Node Consensus Wiring
- ADR-0038: Genesis Format And Node Daemon Bootstrap

Supersedes: None

Referenced By:

- ADR-0042: BlockEnvelope And Receiver-Side Header Verification
  (closes this ADR's own "Explicitly Not Resolved: Receiver-Side
  Header Reconstruction" — a receiver now recomputes and verifies
  `block_hash` rather than trusting a proposer's claimed value)

## Context

`hn_state::BlockHeader`/`BlockBody` (ADR-0008) now exist as concrete,
encodable, hashable Rust types. `hn-node`'s own driving loop (ADR-0037)
still used `synthetic_block_hash(height, round, proposer)` — a plain
concatenation of `height`/`round`/`proposer` bytes, explicitly
documented at the time as "not ADR-0008's real block hash... carries no
cryptographic meaning to verify" — as the value proposed, voted on, and
finalized every round.

The user's own framing, after building `BlockHeader`/`BlockBody`: wire
`block_hash` into the real type where it is actually used. This ADR
records exactly which value each `BlockHeader` field gets in `hn-node`
today, and why — several fields have no real content to compute yet
(no transactions execute, no real validator-set-commitment schema
exists), so this necessarily mixes genuinely-correct values with
clearly-labeled placeholders, not a uniform "real" header end to end.

## Decision

### Decided: `build_block_header` Replaces `synthetic_block_hash`

The current round's proposer (`hn-node/src/node.rs`,
`maybe_start_propose_stage`) now builds a real `hn_state::BlockHeader`
and calls its own `.block_hash()` — the value broadcast in
`ConsensusProposalMessageV1.block_hash` and voted on is a real
`HASH_PROFILE_0x0001("hnchain.block.header.v1", HNCS(BlockHeader))`
digest, not a synthetic one. Field by field:

- `header_version`/`chain_id`/`network_id`/`height`/`round`: real,
  already-available local config/consensus state.
- `epoch = Epoch::new(0)`, `protocol_epoch = ProtocolEpoch::GENESIS`:
  no consensus-protocol epoch rotation or hard-fork/activation
  signaling exists anywhere in this devnet yet — `epoch` already
  matched this exact fixed value in every vote this node casts before
  this pass; not a new placeholder, just now also used here.
- `parent_block_hash`: **real, chained** — `Ctx` gains
  `last_block_hash: Digest`, updated to the just-finalized block's own
  real `block_hash` every time one finalizes, starting from an all-
  zero sentinel before the first block. Genesis's own real
  `parent_block_hash` semantics remain a distinct, separate open
  decision (ADR-0008, "Parent Link": "genesis parent semantics are
  open") — this sentinel is this devnet's own unambiguous "no real
  parent yet" value, not a claim about what a real genesis block's
  field should be.
- `proposer`: this validator's own `validator_id` (`Ctx
  .own_validator_id`) directly — not a computed `address_body`. No
  `validator_address_body` derivation function exists anywhere in
  `hn_crypto` (only `account_address_body` does), and this devnet's
  own `validator_id` is itself "taken as given, never derived"
  (ADR-0038) — there is nothing more "real" to derive it from yet.
- `timestamp`: real wall-clock (`SystemTime::now()`), recorded, not
  validated — this ADR does not touch the still-open general timestamp
  validation window (ADR-0008, "Timestamp"); recording a real local
  reading when *proposing* is a different, much smaller question than
  how (or whether) peers validate it, and does not require that
  question resolved first.
- `transactions_root`/`receipts_root`/`events_root`/`evidence_root`:
  `hn_state::list_empty_root()` — genuinely correct, not simplified:
  this devnet's blocks carry zero transactions by ADR-0037's own
  deliberate scope, so all four are actually empty, not standing in
  for unavailable content.
- `state_root`: **genuinely correct, not a placeholder** — `Ctx` gains
  `state_root: Digest`, computed exactly once at startup
  (`GenesisManifest::initial_state_root`, ADR-0038) and never
  recomputed. Because every block's transaction list is empty,
  `commit_finalized_block`'s own write-set is always empty too, so the
  real state root provably never changes after genesis for as long as
  this remains true — this is the actual value, not a stand-in for a
  real per-block computation this pass skips.
- `consensus_root`: the fixed `DEVNET_VSC` constant `hn-node` already
  used for every vote/QC's own `validator_set_commitment` field before
  this pass. Reusing it here (rather than a second, different
  placeholder) is what keeps ADR-0010's own decided equality —
  `consensus_root` *is* `validator_set_commitment`, the same value,
  not two independently-computed ones — genuinely true in this
  devnet's own data, not just asserted in prose.
- `protocol_parameters_hash`: `hn_state::
  protocol_parameters_placeholder_hash()` — the exact placeholder
  ADR-0008 already decided.
- `extra_data_hash`: `hn_state::extra_data_hash(&[])` — empty
  `extra_data`, matching ADR-0008's own "Decided: Extra Data Format."

### Explicitly Not Resolved: Receiver-Side Header Reconstruction

**Resolved by ADR-0042** ("BlockEnvelope And Receiver-Side Header
Verification"). At the time this ADR was written, a receiving node
still trusted `ConsensusProposalMessageV1.block_hash` exactly as the
proposer sent it. Independently reconstructing the proposer's own
`BlockHeader` to verify the claimed hash would have required the
receiver to already know or trust several proposer-local values the
message did not carry at all (`timestamp`, in particular — no receiver
can reproduce a proposer's own wall-clock reading bit-for-bit). ADR-0042
resolves this not by reconstructing the header from scratch, but by
having the proposer send the whole header (inside a `BlockEnvelope`)
and sign its own `block_hash` — the receiver recomputes the hash from
the received bytes and verifies the signature, so `timestamp` never
needs to be independently reproduced, only accepted (bounded by strict
monotonicity against the receiver's own last finalized block's
timestamp, once that is itself known to be correct). The security
posture this paragraph originally described — "an honest proposer's
claimed hash is now a real, independently-recomputable commitment ...
a necessary foundation for real verification later, not real
verification itself" — is exactly the foundation ADR-0042 built on.

## Rejected Options

### Receiver-Side Header Reconstruction And Verification, This Pass

Rejected: requires either extending `ConsensusProposalMessageV1` to
carry the full header (a real wire-format change beyond "wire
`block_hash` into the real type") or the receiver independently
deriving fields it cannot reproduce (`timestamp` chief among them).
Left as real, separate future work.

### Persisting The Full `BlockHeader`/`BlockBody`

Rejected: nothing in `hn-node` currently reads a block back after
finalizing it (no sync, no light client, no explorer) — only the
`block_hash` value itself is consumed, for `parent_block_hash` chaining
and as the vote/QC target. Storing full headers/bodies with no reader
would be speculative scope, not a real current need.

## Security Considerations

Proposer-controlled `state_root`/`consensus_root`/other placeholder
fields:

- Risk: because several fields are devnet-wide constants
  (`state_root`, `consensus_root`, the two placeholder hashes) rather
  than independently verified per block, a proposer cannot actually
  lie about them in a way any real content would be lost to — there is
  no real content there yet to lie about.
- Mitigation: none needed beyond what is already true: these fields
  are genuinely correct for an always-empty-block devnet, not
  attackable placeholders standing in for real, checkable data.

Unverified `block_hash` claims:

- Risk: a dishonest proposer could send peers a `block_hash` that does
  not match any header it actually holds.
- Mitigation: **closed by ADR-0042** — a receiver now recomputes
  `block_hash` from the proposer's own sent header and verifies a
  signature over it, rather than trusting a bare claimed value. At the
  time this ADR was written, the mitigation was unchanged from
  ADR-0037 (named explicitly above, not a new gap this pass
  introduced).

## Compatibility

No wire-format change: `ConsensusProposalMessageV1`'s own shape
(ADR-0037/ADR-0039) is unchanged: `block_hash` is still just a
`Digest`, now computed differently on the sending side, which no
receiver can observe or needs to know about.

## Open Decisions

- receiver-side header reconstruction and verification — **resolved,
  ADR-0042**
- `BlockEnvelope`/`justification` wiring (ADR-0008) — **resolved,
  ADR-0042**
- real `state_root`/`transactions_root`/`receipts_root`/`events_root`
  once blocks carry real transactions
- real `consensus_root` once `ValidatorSetCommitmentV1`'s own canonical
  byte schema is decided (ADR-0010)
- real `proposer` address derivation for validators (no
  `validator_address_body`-equivalent function exists yet)
- genesis's own real `parent_block_hash` value (ADR-0008, "Parent
  Link," unchanged)

## Related Specifications

- `docs/adr/ADR-0008-block-format.md`
