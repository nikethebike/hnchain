# ADR-0012: Vote Messages And Quorum Certificates

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0011: Leader Election
- ADR-0022: Protocol Versioning

Supersedes: None

## Context

Consensus votes are signed validator statements about proposals, rounds,
timeouts, finality, or other consensus claims.

Quorum certificates aggregate enough valid votes to prove that a consensus
threshold was reached under the active validator set.

HNChain block `justification` cannot be specified safely until vote messages,
vote signing context, quorum thresholds, duplicate handling, and certificate
verification are defined.

## Decision

HNChain defines versioned consensus vote messages and versioned quorum
certificates.

Conceptual vote:

```text
ConsensusVote
  vote_version
  consensus_profile
  vote_type
  chain_id
  network_id
  epoch
  height
  round
  validator_set_commitment
  validator_id
  target_type
  target_hash
  vote_metadata
  signature
```

Conceptual quorum certificate:

```text
QuorumCertificate
  qc_version
  consensus_profile
  certificate_type
  chain_id
  network_id
  epoch
  height
  round
  validator_set_commitment
  target_type
  target_hash
  quorum_threshold
  total_voting_power
  signed_voting_power
  signer_commitment
  aggregate_proof
```

The final signature aggregation scheme is open. Candidate approaches include
individual signatures with bitmaps, batch verification, BLS aggregation, and
threshold signatures.

## Normative Rules

### Versioned Vote Messages

Every vote includes `vote_version`.

Nodes must not infer vote format from signature length, public key algorithm,
network message type, or consensus client version.

### Vote Context Binding

Every vote signature must bind to:

- protocol name
- chain ID
- network ID
- consensus profile
- vote version
- vote type
- epoch
- height
- round
- validator set commitment
- validator ID
- target hash
- signing purpose

Votes valid in one context must not be replayable in another context.

**Decided: vote signing digest mechanism.**

```text
vote_signing_digest = HASH_PROFILE_0x0001(
  "hnchain.vote.signing.v1", HNCS(VoteSigningPayloadV1))
```

Reuses `HASH_PROFILE_0x0001` (ADR-0005) with a domain tag reserved for
this purpose, matching every other signing digest decided so far
(transaction signing, ADR-0006). `protocol_name` and `signing purpose`
in the binding list above are **not** fields of `VoteSigningPayloadV1`
— they duplicate what the domain tag itself already guarantees (same
redundancy class as `protocol_name` inside `TransactionSigningPayload`,
ADR-0006, "Signing Payload," itself mirroring `checksum_profile` inside
`AddressPayload`, ADR-0003 Decision 5): `hnchain.vote.signing.v1`
already says "this digest means HNChain consensus vote signing intent,
and nothing else can produce or accept it," for free. `chain ID` and
`network ID` are the already-decided `chain_id`/`network_id` fields
themselves, not duplicated identifiers.

**Decided: `round` type.** `u64`, matching `hn_core::Round` (already
implemented) and the width convention every other consensus counter in
this project uses (`height`, `epoch`, `protocol_epoch`, `nonce`) — not
narrowed to `u32` despite resetting each height (ADR-0009, "Timeout And
View Change": "a round is one `propose -> prevote -> precommit`
attempt at a fixed height; advancing the round never changes height,"
which is also what fixes `round`'s reset behavior — `Round::FIRST = 0`
at the start of every new height, not a chain-wide monotonic counter
the way `height` itself is). Matching the sibling types' width is
worth more than a narrower bound for a value that, in practice, rarely
grows large: no other counter-like consensus field in this project
uses a narrower width without a specific reason to, and a healthy
network keeps `round` small regardless of its declared type width.

**Decided: `validator_id` width, not its exact derivation.** `bytes32`,
matching every other protocol identifier's width in this project
(`address_body`, `tx_id`, and so on) — but *not* asserted to equal
`hn_crypto::validator_address_body` (which derives from `consensus_key`)
or `account_address`. `validator_id` must stay stable across consensus
key rotation (validator-set.md §4.2, §10, "Key Rotation"), while
`validator_address_body` is derived from the consensus key itself and
would change on rotation — so the two are not obviously the same value,
and deciding which one `validator_id` actually is (or whether it is a
third, separately-assigned value) belongs to ADR-0010's own still-open
`ValidatorRecordV1` closure, not here. Fixing only the width lets
`ConsensusVote`/`QuorumCertificate` be fully encodable now without
guessing at that derivation.

**Decided: `consensus_profile` type.** `u16`, matching the width
convention of every other profile-identifier field in this project
(`hash_profile_id`, `tree_profile`, `LIST_TREE_PROFILE_ID`). `0x0001`
identifies the Tendermint-style profile decided in ADR-0009 (`0x00` is
not reserved here the way `chain_id`/`tx_type` reserve it — this is a
single-value profile identifier like `tree_profile`, not a per-object
registry with a meaningful "absent" case).

**Decided: `epoch` type.** `hn_core::Epoch` (`u64`, already
implemented) — the consensus/validator-set epoch (ADR-0009, "Height,
Round, And Epoch": a consensus-protocol concept), **not**
`protocol_epoch` (`hn_core::ProtocolEpoch`, ADR-0008/ADR-0022):
`ConsensusVote`/`QuorumCertificate` were never conceptually defined
with a `protocol_epoch` field at all (only `epoch`) — an earlier draft
of this section's own prose momentarily conflated the two, the same
class of naming confusion ADR-0008's own `protocol_epoch` gap fix
caught and corrected for `BlockHeader`. A vote binds to the validator
set epoch it was cast under; it has no reason to also carry the
separate hard-fork/activation signal.

With this, `VoteSigningPayloadV1`'s full field list is closed: mirrors
`ConsensusVote` minus `signature` (a signature cannot cover itself) and
minus `protocol name`/`signing purpose` for the reason above. Every
field now has a decided type: this ADR (`vote_version`,
`consensus_profile`, `vote_type`, `epoch`, `round`, `validator_id`),
ADR-0006 (`chain_id`, `network_id`), ADR-0008 (`height`,
`hn_core::BlockHeight`), ADR-0010 (`validator_set_commitment` — the
value itself, `consensus_root`), or is left intentionally generic
(`target_type`/`target_hash` — closed registry and `bytes32` per
"Quorum Certificate Target," below; `vote_metadata` — bounded bytes,
profile-specific by design, not a fixed schema).

### Vote Types

Vote types are consensus-profile specific.

Initial conceptual vote classes:

- `proposal`
- `prevote`
- `precommit`
- `commit`
- `timeout`
- `nil`
- `checkpoint`

The accepted consensus profile must define which vote types are active.

**Decided: `vote_type` registry for Tendermint-style BFT** (ADR-0009,
"Decided: initial consensus family"), `u8`, closed for this profile:

```text
0x00  reserved, invalid
0x01  prevote
0x02  precommit
```

`proposal` is not a `vote_type`: a proposal is the proposer's own
single signed block proposal, not a multi-validator claim requiring
quorum aggregation, so it does not belong in the same registry as
objects that get counted into a `QuorumCertificate`. `commit` is not a
separate vote a validator casts — a block becomes committed when a
`precommit` quorum forms, which is a property of a
`QuorumCertificate.certificate_type` (below), not a vote a validator
sends. `timeout` and `nil` are not separate vote types either: in
classic Tendermint, a validator that cannot vote for a real block at a
round still casts a real `prevote` or `precommit`, just with
`target_type = nil` and an empty `target_hash` — the *type* of the
vote (which round-stage it belongs to) is unchanged, only its target
is. `checkpoint` stays out of this registry for now: checkpointing
(ADR-0016) is a separate mechanism from per-block finality voting and
has not been decided yet — extending this registry for it later needs
no other change here, since `vote_type` is closed only "for this
profile," matching every other closed-for-this-profile registry in
this project.

### Validator Eligibility

A vote is valid only if:

- validator is in the active validator set for the vote context
- validator has nonzero voting power
- validator consensus key is valid for the epoch
- validator status permits voting
- signature verifies under the validator's consensus key

### Duplicate And Conflicting Votes

Duplicate votes from the same validator for the same target are ignored after
the first valid vote.

Conflicting votes from the same validator for the same height, round, vote type,
and safety domain are evidence candidates.

The exact conflict rules must be defined by the consensus profile.

### Quorum Threshold

Quorum calculation must use integer arithmetic.

The threshold must be defined over total voting power for the active validator
set.

For BFT profiles targeting fewer than one third Byzantine voting power, the
expected direction is a threshold greater than two thirds of total voting power.

**Decided: quorum threshold formula.** A certificate meets threshold
when `signed_voting_power * 3 > total_voting_power * 2` — strict
majority above two thirds, using multiplication instead of division to
avoid rounding-mode ambiguity across implementations (matching ADR-0000,
"No Hidden Consensus Dependencies": no floating point, no
implementation-defined rounding). This is independent of the voting
power *model* (ADR-0010, still open — equal-weight, stake-weighted,
capped, or committee-based all produce a `total_voting_power` this same
formula applies to unchanged) and independent of `voting_power`'s
final integer width (ADR-0010, still open) — the formula is exact
integer arithmetic regardless of what width `total_voting_power` and
`signed_voting_power` end up using, as long as `signed_voting_power *
3` cannot overflow that width (a constraint on the chosen width, not on
this formula).

**Decided: `certificate_type` registry for Tendermint-style BFT**, `u8`,
closed for this profile:

```text
0x00  reserved, invalid
0x01  prevote
0x02  precommit
```

Mirrors `vote_type` above — a `QuorumCertificate` aggregates votes of
one type. Only `precommit` certificates are used as finality proof
(ADR-0013, "Finality Proof Binding"); a `prevote` certificate_type
exists in the registry for completeness and local/internal use (a
validator computing its own prevote quorum before precommitting) but is
not itself embedded in a block's `justification`.

### Quorum Certificate Target

A quorum certificate must bind to a single target.

The target may be:

- block hash
- proposal hash
- checkpoint hash
- timeout claim hash
- consensus object hash

The target type must be explicit.

**Decided: `target_type` registry**, `u8`, closed for this profile:

```text
0x00  reserved, invalid
0x01  block
0x02  nil
```

`target_hash` is `bytes32` always (`block_hash`, ADR-0008, when
`target_type = block`; the all-zero digest when `target_type = nil` —
not a variable-length or absent field, so decoding never needs to
branch on `target_type` to know how many bytes follow). `proposal`,
`checkpoint`, and `timeout claim` stay unassigned: "timeout claim" has
no object to target at all (ADR-0009, "Timeout And View Change" — no
separate timeout-certificate object exists), `checkpoint` is deferred
with ADR-0016 (untouched), and "proposal" is not a distinct target
from `block` in this profile — a `prevote`/`precommit` targets the
block a proposal carries, not the proposal message itself, matching
classic Tendermint. `consensus object hash` is not a registry entry;
it was a placeholder category, not a concrete target this profile
uses.

### Signer Commitment

The certificate must commit to the signer set.

The commitment may be represented by a canonical bitmap, sorted signer list,
Merkle root, aggregate signature metadata, or another specified format.

The final representation must support deterministic verification and malformed
signer rejection.

### Aggregation

Signature aggregation is an optimization, not a hidden consensus rule.

The certificate must define enough data to verify:

- signer eligibility
- signer uniqueness
- signed voting power
- target binding
- signature validity
- threshold satisfaction

### Light-Client Verification

Quorum certificates must be verifiable by light clients with the validator set
commitment and required validator set proof.

Light clients must not trust RPC assertions that a block is finalized without
verifying the certificate or an accepted checkpoint proof.

### Evidence

Votes must be canonical enough to serve as evidence.

Evidence validity must not depend on local logs, mempool state, network arrival
order, or operator testimony.

## Rejected Options

### Unversioned Votes

Rejected because consensus message formats must evolve over decades without
ambiguous parsing.

### Vote Signatures Without Context

Rejected because signatures could be replayed across vote types, rounds,
heights, chains, or networks.

### Floating-Point Quorum Calculation

Rejected because consensus arithmetic must be deterministic across platforms.

### Quorum Certificate Without Signer Set

Rejected because nodes must verify signer uniqueness and voting power.

### RPC Finality Flag As Consensus Proof

Rejected because RPC responses are not consensus objects.

## Alternatives Considered

### Individual Signatures With Bitmap

Advantages:

- simple to reason about
- no special aggregation cryptography
- strong compatibility with algorithm agility

Disadvantages:

- larger certificates
- more verification work
- less efficient for large validator sets

### Batch Verification

Advantages:

- keeps individual signatures
- can improve verification performance
- simpler than aggregate-signature consensus

Disadvantages:

- still large on the wire
- batch failure handling must be deterministic
- algorithm support varies

### BLS Aggregate Signatures

Advantages:

- compact certificates
- efficient light-client proofs
- good fit for large validator sets

Disadvantages:

- adds pairing-based cryptographic assumptions
- requires careful rogue-key protection
- post-quantum migration needs separate analysis

### Threshold Signatures

Advantages:

- compact finality proof
- can hide signer set details if designed that way

Disadvantages:

- complex distributed key management
- harder validator churn
- difficult accountability unless signer evidence is preserved

## Security Considerations

Replay attacks:

- Risk: a valid vote is reused in another context.
- Mitigation: mandatory signing context and domain separation.

Equivocation:

- Risk: validator signs conflicting votes.
- Mitigation: canonical vote evidence and deterministic conflict rules.

Quorum inflation:

- Risk: duplicate validators or malformed signer sets inflate voting power.
- Mitigation: canonical signer commitment, duplicate rejection, and validator
  set commitment verification.

Rogue-key attacks:

- Risk: aggregate signature schemes are abused by malicious key registration.
- Mitigation: proof-of-possession or scheme-specific registration rules if BLS
  or similar aggregation is selected.

Certificate bloat:

- Risk: large certificates harm block propagation and light-client usability.
- Mitigation: size limits, aggregation evaluation, and benchmarked formats.

Long-range attacks:

- Risk: old validator keys sign alternative certificates.
- Mitigation: validator set history, unbonding windows, checkpoints, and
  light-client security rules.

Ambiguous finality:

- Risk: different clients interpret certificate types differently.
- Mitigation: explicit certificate type, target type, and profile version.

## Compatibility

Changing vote signing context, quorum threshold, certificate target semantics,
or aggregation scheme is a major consensus change.

Adding a vote type can be compatible only if:

- vote type is registered
- signing context is defined
- unsupported nodes reject it deterministically
- activation rules are explicit

Changing signature aggregation requires cryptographic identity and light-client
compatibility analysis.

## Open Decisions

- final quorum certificate format (`certificate_type` registry and
  threshold formula decided above; `signer_commitment`/aggregation
  representation still open, below)
- signer commitment representation
- signature aggregation scheme
- batch verification rules
- evidence conflict rules
- checkpoint certificate semantics
- light-client validator set proof format
- maximum vote size
- maximum certificate size

## Related Specifications

- `docs/rfc/consensus/vote-messages-and-quorum-certificates.md`
- `docs/rfc/consensus/finality-rules.md`
