# ADR-0049: Proposer Re-Proposal Wire Support

Status: Accepted

Date: 2026-10-26

Version: 0.1.0

Depends On:

- ADR-0002: Signature Envelope Format
- ADR-0006: Chain And Network Binding
- ADR-0008: Block Format
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0022: Nested Structure Versions
- ADR-0039: Devnet Restart Recovery
- ADR-0042: BlockEnvelope And Receiver-Side Header Verification
- ADR-0048: Byzantine-Safe Consensus — Lock/Valid State, Vote Journal,
  Equivocation Evidence, Round Skipping

Supersedes: None (narrows part of ADR-0042's own "Decided:
`ConsensusProposalMessageV1` Carries A Signed `BlockEnvelope`, Not A
Bare Claim" — see "Context" below — everything else in ADR-0042/0039
stays in force)

Referenced By: None yet

## Context

ADR-0048, "Decided: Proposer Duties," decided R1: "The proposer
re-proposes `valid_value` with its polka when it has one... This is
the fix for Finding 3.1." `hn-node`'s own proposer has never
implemented this (it always sends `justification: None`, confirmed
directly in `maybe_start_propose_stage` and recorded in ADR-0048's own
commit history and in ADR-0042's "Open Decisions") — ADR-0048 itself
scoped the wiring as later work, and the user chose, while that later
work was actually being attempted, to scope the wire-protocol piece
below into its own ADR rather than fold it into an internal-only
change.

The blocker is structural, not an oversight: `block_hash` is
`HASH_PROFILE_0x0001("hnchain.block.header.v1", HNCS(BlockHeader))`
(ADR-0008) — a commitment to the *entire* header, `round` included.
R3's own justification check requires `justification.target_hash ==
block_hash`, i.e. the re-proposed value must hash *identically* to
what the original polka certified. The only way to re-propose "the
same value" at a new round with an unchanged `block_hash` is to
resend the original `BlockEnvelope` completely unchanged — including
its own `header.round`, still naming the round the value was first
built for, not the round this delivery is actually for.

ADR-0042 removed exactly this kind of "second round value" from
`ConsensusProposalMessageV1` on purpose: "The old top-level
`height`/`round`/.../fields are gone: `envelope.header.height`/`.round`
already self-describe the target... a redundant top-level copy would
just be a second value that could disagree with the header's own."
That reasoning holds for every proposal ADR-0042 itself considered,
because at the time, a proposal's own round and its header's round
were always the same value by construction. R1 breaks that
invariant on purpose: a re-proposal's delivery round and its header's
own (original) round are now two genuinely different, independently
meaningful values, and nothing on the wire names the first one.

## Decision

### Decided: `ConsensusProposalMessageV1` Gains `for_round`

```rust
pub struct ConsensusProposalMessageV1 {
    pub envelope: BlockEnvelope,
    pub for_round: Round,
    pub proposer_signature: SignatureEnvelope,
}
```

`for_round` is the round this specific delivery claims to be for.
For a fresh proposal (`envelope.justification: None`), `for_round ==
envelope.header.round` always — the common case is unchanged in
substance, just now explicit on the wire instead of implicit.
For a re-proposal (`envelope.justification: Some(qc)`), `envelope` is
resent byte-for-byte from whenever it was first built, `envelope.
header.round` stays at that original round, and `for_round` is this
validator's own current round. `envelope.header.height`/`block_hash`
are unaffected; this is the one and only field this ADR adds.

### Decided: `ProposalRoundClaimV1` — A New Signing Payload

ADR-0042's own `proposer_signature` signs `envelope.block_hash()`
directly, reasoning that `block_hash` is already a real,
domain-separated, unambiguous 32-byte commitment to the entire header,
so a second wrapper struct would duplicate it for no benefit. That
reasoning no longer holds once `for_round` exists: `block_hash` says
nothing about `for_round` at all, so signing `block_hash` alone would
let *anyone* relay a validator's own old, legitimately-signed envelope
and attach an arbitrary `for_round` to it. Concretely: `round_proposer`
assigns the same validator every `n` rounds (`(height + round) % n`),
so a relay could take validator `V`'s real round-0 proposal and
re-broadcast it claiming `for_round = n`, where `round_proposer` again
names `V` — passing proposer-eligibility, passing R3's justification
check (`0 < n`), and passing signature verification, all without `V`
ever deciding to re-propose anything at round `n`. `for_round` must
be inside what the proposer actually signs.

```rust
pub struct ProposalRoundClaimV1 {
    pub chain_id: u8,
    pub network_id: u16,
    pub for_round: Round,
    pub block_hash: Digest,
}

impl ProposalRoundClaimV1 {
    pub fn signing_digest(&self) -> StateResult<Digest> {
        Ok(hash_profile_0x0001(
            "hnchain.proposal.roundclaim.v1",
            &self.encode()?,
        )?)
    }
}
```

Mirrors `VoteSigningPayloadV1::signing_digest`'s own exact shape
(ADR-0012) — a dedicated struct, its own canonical encoding, its own
domain-separated hash profile tag, chain/network binding included
(ADR-0006), not a second, implicit payload (ADR-0002, "No Implicit
Signing Payloads"). `proposer_signature` in `ConsensusProposalMessageV1`
now verifies against this digest, not against bare `block_hash`.
`height` is not a separate field here: it is already inside
`block_hash` (the whole header, including height, is hashed into it),
so binding `block_hash` already binds height transitively; nothing
about `for_round` needs height repeated next to it.

### Decided: Receiver-Side Verification Changes

Of ADR-0042's own seven `verify_proposal` steps, exactly two change;
the rest are unchanged (with `header` always meaning `envelope.header`
below):

- Step 3 (proposer eligibility) uses `for_round`, not `header.round`:
  `round_proposer(ordered_ids, header.height, for_round) ==
  header.proposer`. This is the whole reason `for_round` must be
  signed — eligibility is decided for the round the message actually
  claims to be for.
- Step 4 (signature) verifies `proposer_signature` against
  `ProposalRoundClaimV1 { chain_id, network_id, for_round, block_hash
  }.signing_digest()`, replacing the bare `block_hash` digest.

`envelope.verify_body_matches_header()`, the fixed-value header checks,
`parent_block_hash`/timestamp monotonicity, and embedded-justification
signature verification (steps 1-2, 5-7) are unchanged — none of them
ever depended on which round the message was delivered for.

### Decided: `ConsensusEngine::handle_proposal`'s Own `round` Argument

`hn-node` passes `for_round` (not `header.round`) as the `round`
argument to `ConsensusEngine::handle_proposal` (the explicit parameter
ADR-0048's own phase-4 work added specifically so the caller's real
claimed round reaches the state machine, rather than the engine's
current round silently standing in for it). This was already true in
shape before this ADR — `for_round` simply replaces what was, before
this ADR existed, the only round value on the message
(`header.round`) at that one call site.

### Decided: Proposer-Side Re-Proposal Logic

In `maybe_start_propose_stage`, before building a fresh block, check
this engine's own `valid_value`/`valid_round`/`valid_qc`. If all three
are `Some` **and** this validator's own `pending_envelopes` cache
still holds the matching `BlockEnvelope` (keyed by `valid_value`'s own
hash):

- Resend that exact envelope unchanged (`justification:
  Some(valid_qc.clone())` is already how it was cached — ADR-0048's
  own `ConsensusState.valid_qc` is already set to exactly the polka
  this needs).
- `for_round` = this engine's own current round.
- Build a fresh `ProposalRoundClaimV1`/`proposer_signature` for this
  specific `(for_round, block_hash)` pair — the signature itself
  cannot be reused across rounds even though the envelope is.

If `pending_envelopes` does **not** have it (this validator restarted
and its vote journal — ADR-0048's own rule 1 — restored `valid_value`/
`valid_qc` but names no proposal content at all to repeat), fall back
to building a fresh block for the current round instead, same as
having no `valid_value` at all. This is a documented liveness
trade-off, not a safety gap: proposing a fresh value for the *current*
round never conflicts with "a proposer may not send two different
proposals for the same (height, round)" (ADR-0048), since nothing was
ever sent for this round before. It can cost a round if enough other
validators are themselves still locked on the lost value, exactly the
same outcome as if this validator had not been the proposer at all
that round.

## Rejected Options

### Deriving `for_round` From The Embedded `justification.round + 1`, Or Similar

Rejected: a re-proposal is not required to happen at `valid_round +
1` specifically — a validator can fall behind, skip rounds (R10, once
it exists), or simply be proposer again several rounds later. `for_round`
must be the real, current round, carried explicitly, not reconstructed
from an unrelated field that happens to often be close to it.

### Signing `(block_hash, for_round)` As A Raw Concatenated Byte String Instead Of A New Struct

Rejected: ADR-0002's own "No Implicit Signing Payloads" rule exists
exactly to prevent ad hoc concatenations whose own field boundaries
and canonical form are not independently defined anywhere. `VoteSigningPayloadV1`
already established the right shape for "a new thing this codebase
signs" in this exact domain; `ProposalRoundClaimV1` reuses it rather
than inventing a second convention.

### Keeping `block_hash` As The Only Signed Value And Checking `for_round` Only Structurally

Rejected (see "Decided: `ProposalRoundClaimV1`" above for the concrete
replay this would allow): `round_proposer`'s own periodic
proposer-reuse makes an unsigned `for_round` field forgeable into a
believable-looking re-proposal the real proposer never made.

## Compatibility

Breaking change to `ConsensusProposalMessageV1`'s own wire shape —
the same precedent ADR-0039 and ADR-0042 already each set for this
exact type, acceptable pre-mainnet. No change to `BlockHeader`/
`BlockBody`/`BlockEnvelope`'s own shape. `hn_consensus::ConsensusEngine::
handle_proposal`'s own signature is unchanged (it already takes an
explicit `round` since ADR-0048's phase-4 work); only what `hn-node`
passes into it changes.

## Security Considerations

Closed by this ADR:

- The `for_round` replay this ADR's own "Decided: `ProposalRoundClaimV1`"
  section names explicitly — now cryptographically impossible, since
  `for_round` is part of what the proposer actually signs.

Explicitly not closed, named rather than silently assumed:

- A proposer that is slow, or dishonestly withholds a re-proposal it
  could legitimately send, is a liveness question (ADR-0048's own
  "Decided: Gossip Requirements Safety Depends On" already names the
  broader re-broadcast-on-a-timer gap as deferred to a future P2P
  ADR) — unaffected by this one.
- R10 (round skip from gossiped evidence) remains separately deferred
  — unrelated to this ADR's own scope, named here only so neither gap
  is mistaken for the other.

## Open Decisions

- R10 (round skip from gossiped evidence) — a distinct, separate wire
  addition, not attempted here.
- The liveness cost named in "Decided: Proposer-Side Re-Proposal
  Logic" when a restarted proposer's own `pending_envelopes` cache
  lost the value its vote journal still names as `valid_value` — a
  real, accepted trade-off, revisit only if it proves costly in
  practice.

## Related Specifications

- `docs/adr/ADR-0048-byzantine-safe-consensus.md`
- `docs/adr/ADR-0042-block-envelope-and-receiver-side-header-verification.md`
