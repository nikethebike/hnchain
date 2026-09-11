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
  total_voting_power
  signed_voting_power
  signer_commitment
  aggregate_proof
```

`quorum_threshold` (originally listed above) is dropped: the quorum
threshold formula is fixed for this profile (Decision, below —
`signed_voting_power * 3 > total_voting_power * 2`), not a
per-certificate configurable value, so a stored field would duplicate
what `total_voting_power` already determines on its own. Same
redundancy class as `protocol_name` (`TransactionSigningPayload`,
ADR-0006), `checksum_profile` (`AddressPayload`, ADR-0003 Decision 5),
and `hash_profile` (`ValidatorSetCommitmentV1`, ADR-0010) — the fifth
instance of this pattern found this session.

**Decided: individual signatures with bitmap.** Asked the user first — same
weight as the consensus family and voting power model choices, per the
user's own framing: the final QuorumCertificate shape hinges on it, and it
was deliberately left undecided (not guessed) while implementing
`ConsensusVote`.

Checked against ADR-0002 (Cryptographic Identity, Accepted) before treating
this as a free three-way choice, and found it narrows sharply: ADR-0002's
"Accepted Initial Direction" already fixes `validator_consensus` to
Ed25519 as "the primary and only active consensus signing suite" at
genesis — BLS isn't even in the reserved-but-inactive list (secp256k1,
Ed448, ML-DSA, SLH-DSA are). BLS-style pairing aggregation does not work
on Ed25519 signatures at all; taking it would mean reopening an Accepted
ADR to activate a new genesis algorithm, directly against that ADR's own
stated rationale ("minimizes the active consensus surface"). Threshold
signatures could in principle stay Ed25519-compatible (e.g. FROST), but
this ADR's own Alternatives Considered already names "harder validator
churn" as threshold's disadvantage — and validator churn is not
hypothetical here: ADR-0010's already-decided epoch-based active-set
rotation changes the set every epoch, which would force threshold
resharing on a cadence the protocol already committed to.

`QuorumCertificate.aggregate_proof` is a list of individual Ed25519
signatures, one per signer, `signer_commitment` a bitmap over the active
set identifying which validators signed. No new signing algorithm, no new
dependency; batch verification (verifying many Ed25519 signatures faster)
remains available as a pure implementation-level optimization on top of
this format later — it does not require a different certificate structure,
so it is not a competing branch (Alternatives Considered, below). Certificate
size is `O(n)` in active set size, acceptable while ADR-0010's "initial
active validator set size policy" (still open) stays modest; revisiting
aggregation later, if the active set grows large, would need its own
ADR-0002 amendment at that time, not a decision made now against an
unknown target size.

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

**Decided: `validator_id` width, not its exact derivation (derivation
since resolved in ADR-0010).** `bytes32`, matching every other protocol
identifier's width in this project (`address_body`, `tx_id`, and so
on) — deliberately *not* asserted here to equal
`hn_crypto::validator_address_body` (which derives from `consensus_key`
and would change on key rotation), since `validator_id` must stay
stable across rotation (validator-set.md §4.2, §10, "Key Rotation").
Fixing only the width let `ConsensusVote`/`QuorumCertificate` be fully
encodable at the time without guessing at that derivation. ADR-0010's
own `ValidatorRecordV1` closure has since settled it ("Decided:
`validator_id` derivation"): the controlling account's own
`address_body`, not `validator_address_body` — confirming the two
really were different values, as this paragraph originally suspected.

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
implementation-defined rounding). This was decided independently of the
voting power *model* and of `voting_power`'s integer width, both open at
the time (ADR-0010) — since resolved (capped stake-weighted, `u128`),
confirming the formula needed no revision: it is exact integer
arithmetic regardless of what width `total_voting_power` and
`signed_voting_power` use, as long as `signed_voting_power *
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

The commitment is a bitmap over the active validator set (Decision, above —
"individual signatures with bitmap"), not an aggregate-signature-derived
representation.

**Decided: signer commitment bit-level encoding.** Directly continues the
"bitmap" kind decision above — ordering and width are the only two things
that decision left unresolved, and both are derivable from already-decided
material rather than independent forks:

- **Ordering.** Bit `i` corresponds to the `i`-th validator in the same
  canonical order `validators_root` already uses — ascending `validator_id`
  (ADR-0010, "Decided: `validators_root` and `validator_set_commitment` /
  `consensus_root` mechanism"). A verifier that checks a `ConsensusVote`'s
  signer against the active set already reconstructs this exact ordered
  list to walk `validators_root`'s Merkle structure; reusing it for the
  bitmap means no second canonical ordering exists anywhere in the protocol
  for the same active set. Inventing a different order here (e.g. by
  registration time or descending voting power) would buy nothing and cost
  every verifier a second ordered index to maintain.
- **Width.** The bitmap's byte length is `ceil(active_set_size / 8)`,
  where `active_set_size` is the cardinality of the active validator set
  for the epoch the vote/certificate references — data every verifier
  already has, independent of whether `ValidatorSetCommitmentV1` itself
  ever gains a dedicated `validator_count` field (`validator-set.md` §7's
  conceptual struct has one, but that field's presence is still open —
  "final validator set commitment format," ADR-0010's RFC). On the wire,
  `signer_commitment` is still an HNCS bounded `bytes` value (like
  `vote_metadata`), not a protocol-wide fixed-size field — the active set
  changes size across epochs, so no single width constant could cover
  every epoch. What is fixed is the check: a decoder must reject any
  `signer_commitment` whose decoded byte length is not *exactly*
  `ceil(active_set_size / 8)` for the referenced epoch, not merely accept
  whatever length the field happens to carry.
- **Bit packing.** Byte index `floor(i / 8)`, bit position `i mod 8` within
  that byte, least-significant-bit first — `bit 0` of `byte 0` is
  validator index `0`, `bit 7` of `byte 0` is validator index `7`, and so
  on. LSB-first keeps the same "lowest index, least significant" mental
  model HNCS already uses for little-endian multi-byte integers (ADR-0004),
  rather than mixing bit- and byte-order conventions within one structure.
  Any padding bits beyond `active_set_size - 1` in the final byte (when
  `active_set_size` is not a multiple of 8) must be zero; a decoder must
  reject a `signer_commitment` with any padding bit set, per the malformed
  signer commitment rejection already required below.

A concrete `MAX_SIGNER_COMMITMENT_LEN`-style implementation bound (the same
class of decision as `MAX_VOTE_METADATA_LEN`/`MAX_VOTE_SIGNATURE_LEN`,
picked with headroom ahead of the final protocol parameter) is deferred to
implementation time, once ADR-0010's "maximum active set size, if any" gives
a concrete number to size it against — the same relationship
`OBJECT_ID_MAX_LEN` already has with ADR-0003's final address body length.

The final representation must support deterministic verification and malformed
signer rejection.

### Aggregation

Signature aggregation is an optimization, not a hidden consensus rule.

No signature-level aggregation is performed under the decided scheme
(Decision, above): verification checks each individual Ed25519 signature
named by the signer bitmap. "Aggregation" in this profile refers only to
possible future verification-time batching, never to a change in what the
certificate carries.

The certificate must define enough data to verify:

- signer eligibility
- signer uniqueness
- signed voting power
- target binding
- signature validity
- threshold satisfaction

**Decided: `vote_metadata` must be empty for any vote eligible to be
certified.** Found while implementing verification, not while
designing the wire format: checking "signature validity," above,
requires a verifier to reconstruct exactly what signer `i` originally
signed — a full `VoteSigningPayloadV1` — from the certificate's own
fields. Every field of that reconstruction is already determined by
the certificate except `vote_metadata` (`chain_id`/`network_id`/
`epoch`/`height`/`round`/`validator_set_commitment`/`target_type`/
`target_hash` come directly from the certificate; `vote_type` is
`certificate_type`; `validator_id` is the signer named by the relevant
`signer_commitment` bit) — a `QuorumCertificate` does not preserve each
signer's `vote_metadata`, so if two signers' original votes used
different `vote_metadata`, nothing in the certificate would tell a
verifier which bytes either of them actually signed.

Resolved by constraining which votes are eligible for certification,
not by changing what a certificate carries: a vote with non-empty
`vote_metadata` may still be validly signed and broadcast, but must not
be counted toward any `QuorumCertificate`'s `signed_voting_power` — an
aggregator collecting votes to build a certificate excludes any vote
whose `vote_metadata` is non-empty, and a verifier reconstructs every
signer's payload with `vote_metadata = []` unconditionally. This
leaves `ConsensusVote`/`VoteSigningPayloadV1`'s own wire format
untouched (`vote_metadata` stays a real, bounded field — every
already-oracle-verified encode/decode test vector for `ConsensusVote`
is unaffected) and keeps `QuorumCertificate.aggregate_proof` exactly
`Vec<SignatureEnvelope>`, not a larger structure carrying per-signer
metadata. The real cost: `vote_metadata` becomes effectively unusable
for prevote/precommit votes in the actual consensus path, meaningful
only for a vote type that never aggregates into a certificate, if one
is ever added.

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

**Selected** (Decision, above). Directly compatible with ADR-0002's
Accepted, Ed25519-only `validator_consensus` suite — the only candidate
here that requires no change to an already-Accepted document. Its
disadvantages are accepted deliberately, bounded by ADR-0010's still-open
"initial active validator set size policy" staying modest for v0.1.

### Batch Verification

Advantages:

- keeps individual signatures
- can improve verification performance
- simpler than aggregate-signature consensus

Disadvantages:

- still large on the wire
- batch failure handling must be deterministic
- algorithm support varies

Not a competing certificate format (Decision, above): batch verification is
an implementation-level optimization that can apply on top of "Individual
Signatures With Bitmap" without changing `QuorumCertificate`'s structure.
Left fully open (Open Decisions, below) as its own later, independent
decision.

### BLS Aggregate Signatures

Advantages:

- compact certificates
- efficient light-client proofs
- good fit for large validator sets

Disadvantages:

- adds pairing-based cryptographic assumptions
- requires careful rogue-key protection
- post-quantum migration needs separate analysis

**Not chosen for v0.1** (Decision, above): BLS is not an active or even
reserved algorithm under ADR-0002 (Accepted) — its reserved-but-inactive
list names secp256k1, Ed448, ML-DSA, and SLH-DSA, not BLS. Adopting it now
would mean reopening an Accepted ADR to activate a new genesis algorithm,
against that ADR's own "minimizes the active consensus surface" rationale.
Not ruled out permanently: if the active validator set later grows large
enough that certificate size becomes a real problem, this can be revisited
as its own ADR-0002 amendment at that time.

### Threshold Signatures

Advantages:

- compact finality proof
- can hide signer set details if designed that way

Disadvantages:

- complex distributed key management
- harder validator churn
- difficult accountability unless signer evidence is preserved

**Not chosen** (Decision, above): "harder validator churn" is not a
theoretical concern here — ADR-0010's already-decided epoch-based active-set
rotation changes the validator set every epoch, which would force threshold
resharing on a schedule the protocol already committed to.

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
- Mitigation: not applicable under the decided scheme (individual signatures,
  Decision above) — each signature verifies independently against its own
  validator's key, so there is no aggregate public key to forge a rogue
  contribution against. Proof-of-possession would become necessary again if
  BLS or similar aggregation is adopted in a future revision.

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

- `MAX_SIGNER_COMMITMENT_LEN`-style implementation bound (encoding
  mechanism decided above; sizing it needs ADR-0010's "maximum active
  set size, if any," still open — `QuorumCertificate` itself is now
  fully decided and encodable: `certificate_type` registry, threshold
  formula, aggregation scheme, signer commitment encoding, and voting
  power's integer type — `u128`, ADR-0010 — are all decided)
- batch verification rules (optional layer on top of the decided
  scheme, not a format fork — see "Batch Verification," Alternatives
  Considered)
- evidence conflict rules
- checkpoint certificate semantics
- light-client validator set proof format
- maximum vote size
- maximum certificate size

## Related Specifications

- `docs/rfc/consensus/vote-messages-and-quorum-certificates.md`
- `docs/rfc/consensus/finality-rules.md`
