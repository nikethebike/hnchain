# HNChain Core Specification: Genesis Security

Status: Draft

Version: 0.1.0

Date: 2026-09-18

## 1. Scope

This document specifies custody, key generation, multi-party authorization,
and vesting/timelock practices for the three genesis allocation accounts
defined by `docs/adr/ADR-0024-hncoin-monetary-policy.md`: the Liquidity &
Ecosystem Reserve, the Founder Allocation, and the Community & Airdrop
Allocation.

It defines *who may authorize a transaction from each account, under what
process, and by when funds become eligible to move*. It does not redefine
the allocation amounts, which are fixed by ADR-0024.

Explicitly out of scope, each a separate, still-open item (ADR-0024, Open
Decisions):

- the Community & Airdrop Allocation's own end-recipient distribution
  mechanism and schedule (this document covers custody of the account
  before distribution, not how it reaches end users)
- the Liquidity & Ecosystem Reserve's spending authorization and use-case
  approval process (this document covers who may authorize *a* transaction,
  not what the Reserve's funds may be spent on)
- the general account-level multisignature/Permission model (ADR-0002;
  `docs/specs/core/account-state.md` §4.5) — see §2, below, for why this
  document does not depend on that model existing

This specification is constrained by:

- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0003-address-format.md`
- `docs/adr/ADR-0024-hncoin-monetary-policy.md`
- `docs/specs/core/genesis.md`
- `docs/specs/core/account-state.md`
- `docs/security/README.md`

## 2. Why Procedural, Not Protocol-Enforced

No on-chain threshold or multisignature verification rule exists anywhere
in this project today. ADR-0002's "Accepted Initial Direction" requires
exactly one active signing key per role "unless the owning object
specification defines a threshold or multisignature rule" — no object
anywhere defines one yet. `SignatureEnvelope` verification derives the one
expected key directly from state (`key = state.active_key(identity, role,
height)`), not from a declared set of authorized keys with a pass
threshold. Account-level Permission state (`account-state.md` §4.5, which
would be the natural home for such a rule) is explicitly "not activated by
this specification... each permission feature requires its own dedicated
state transition rule" and has no concrete driver yet. There is also no
HNVM/smart-contract layer (still undesigned) to build an on-chain
multisig wallet contract on top of, the way many other chains implement
custody multisig without a protocol-level primitive at all.

**Decided**: genesis-account custody and vesting for v1 are enforced
**procedurally, outside the protocol**, not by a consensus rule. Every
transaction these three accounts ever send is an ordinary single-signature
transaction, structurally indistinguishable on-chain from any other
account's transaction. The "M-of-N" discipline described below is a
property of the offline process that produces that one signature, not
something a validator or light client can verify. This is a deliberate v1
scope choice, not an oversight — introducing real on-chain enforcement
would require designing and activating a general threshold/multisignature
rule (extending `SignatureEnvelope` verification and reintroducing
`key_reference`, per ADR-0002's own already-anticipated path) and,
separately, a height-locked vesting primitive comparable in shape to
`ValidatorRecordV1.pending_unbonding` (ADR-0010/ADR-0023) but for ordinary
account balances — both real protocol features, not policy documents, and
both left for a dedicated future ADR if this project decides the need
justifies the scope. This document does not foreclose that; it documents
the v1 procedural baseline it replaces.

## 3. Threat Model

- Compromise of a single custodian's key share or device.
- Loss of a custodian's key share or device (with no recovery procedure).
- Collusion among a threshold-sized subset of custodians.
- Coercion of a threshold-sized subset of custodians.
- Premature or unauthorized spending ahead of a published vesting schedule.
- Inability for the community to verify that custody practice actually
  matches what was published (no protocol-level attestation mechanism).
- Transcription or tooling error during key generation producing an
  address the intended custodians do not actually control.

## 4. Custody Model Overview

The on-chain signing key for each of the three accounts is a single
Ed25519 keypair — no different in wire shape from any other
`account`-namespace address (ADR-0003, ADR-0002). No custodian, and no
subset of custodians below the account's own threshold, ever holds
complete usable private key material outside of an active, witnessed
signing session.

Two acceptable off-chain patterns for producing that one signature (an
account may use either; the choice and exact configuration must be
documented at key-generation time, §7):

- **Secret-sharing reconstruction** (e.g. Shamir's Secret Sharing): the
  Ed25519 private key is split into `N` shares at generation time; `M`
  shares are required to reconstruct it. Reconstruction happens only
  inside an air-gapped environment for the duration of producing a single
  signature, after which the reconstructed key material is destroyed
  again — never persisted whole.
- **Coordinated co-signing ceremony**: `M` of `N` custodians, each holding
  an independent hardware-wallet-protected credential, follow a documented
  manual review-and-approve process to jointly authorize a signing
  coordinator (a role, not necessarily a single perpetual person) to
  produce the one on-chain signature. Internal tooling is an
  implementation detail, not a consensus concern.

Either pattern yields the same chain-observable result: one ordinary
Ed25519 signature from one account address.

## 5. Per-Account Custody Requirements

The three accounts do not share identical risk profiles (different held
amounts, different expected transaction frequency), so each gets its own
threshold policy rather than one blanket rule.

### 5.1 Liquidity & Ecosystem Reserve (80% of supply)

- Highest custody bar of the three — holds 8× either other allocation.
- Threshold `M`-of-`N`: **open** (Open Decisions, below).
- Hardware-wallet-protected credentials required for every custodian
  share, no exception.
- Custodian organizational/geographic diversity policy: recommended in
  principle (no single organization or jurisdiction should be able to
  reach the threshold alone); exact policy **open**.
- Spending authorization / use-case approval process for what the Reserve
  may fund: explicitly **not** this document's scope — a separate
  ADR-0024 Open Decision.

### 5.2 Founder Allocation (10% of supply)

- Threshold `M`-of-`N`: **open**.
- Hardware-wallet-protected credentials required for every custodian
  share.
- Subject to the vesting schedule in §6: the account may hold
  vested-but-unspent HNCOIN at any time. The custody threshold governs
  *who may authorize moving any amount at all*; vesting governs *how much
  is legitimate to move by when*. The two are independent controls, both
  required.

### 5.3 Community & Airdrop Allocation (10% of supply)

- Threshold `M`-of-`N`: **open** — plausibly a lower-friction
  configuration than Reserve/Founder, since this account is expected to
  send frequent, small, operational distribution transactions rather than
  rare large ones; exact policy **open**.
- The mechanism and schedule by which this account's funds actually reach
  end recipients (airdrop criteria, claim process, timing) is explicitly
  **not** this document's scope — a separate ADR-0024 Open Decision. This
  document covers only custody of the account's funds prior to
  distribution.

## 6. Vesting (Procedural)

The Founder Allocation is subject to a publicly documented release
calendar, not a consensus-enforced lock — no protocol mechanism exists to
prevent an early transfer (§2). The custodians named in §5.2 commit, as
part of accepting the custodian role, to only co-sign or contribute a key
share toward a Founder-account transfer that matches the published
schedule.

- The schedule's own shape (cliff length, total vesting duration, release
  curve — linear vs. milestone-based) is **not decided** by this document
  — Open Decisions, below.
- The release calendar must be published before genesis, alongside the
  document set genesis already commits to (`docs/specs/core/genesis.md`
  §5–§6, "Genesis Manifest" / "Document Commitments"), so the community
  can independently observe whether on-chain withdrawals from the Founder
  address match the public commitment.
- Enforcement is reputational and social, not cryptographic: nothing stops
  custodians who choose to violate the published schedule from doing so.
  This is stated plainly as a residual risk (§8), not hidden behind the
  existence of a published calendar.
- The Liquidity & Ecosystem Reserve and Community & Airdrop Allocation are
  not, by this document, subject to a vesting schedule — the Reserve's own
  spending is instead gated by its (separately open) use-case approval
  process, and Community & Airdrop's release is gated by its (separately
  open) distribution mechanism, neither of which is a time-based vest.

## 7. Key Generation Ceremony Requirements

- Key generation happens offline, in an air-gapped environment.
- The entropy source must be independently verifiable, not a single
  opaque hardware RNG trusted blindly.
- Multiple witnesses must be present for each account's ceremony, and the
  ceremony's occurrence (participants, date, high-level procedure — not
  key material) must be documented.
- No single individual should hold complete, reconstructable key material
  outside an active signing session for a threshold-worthy account (§4).
- The resulting public key and derived address (ADR-0003
  `account_address_body`) must be published before genesis and
  independently recomputed by more than one witness from the same public
  key, catching a transcription or tooling error before it becomes
  irreversible — the same class of check ADR-0003's own address
  derivation work already relies on address recomputation to catch.

## 8. Security Considerations

Single-custodian key compromise:

- Risk: one custodian's key share or device is compromised.
- Mitigation: below the account's own threshold, a single compromised
  share cannot authorize a transaction (§4). Above-threshold compromise is
  a separate risk, below.

Threshold-sized collusion or coercion:

- Risk: an attacker compromises, colludes with, or coerces `M` or more
  custodians of a given account.
- Mitigation: custodian selection diversity (organizational, geographic,
  no single controlling entity able to reach the threshold alone) reduces
  the practical likelihood; there is no cryptographic mitigation, since
  enforcement is entirely procedural (§2). Exact selection policy is an
  Open Decision, below.

Premature vesting release:

- Risk: Founder-account custodians sign a transfer that does not match the
  published release calendar.
- Mitigation: reputational/social only (§6) — public verifiability lets
  the community detect a violation after the fact, but nothing prevents
  it in advance. Recorded as a known, accepted limitation of the
  procedural (non-protocol-enforced) v1 model, not a gap this document
  silently overlooks.

Custodian loss or unavailability:

- Risk: enough custodians lose their key share, device, or availability
  that the remaining custodians fall below the account's own threshold,
  making the account's funds permanently unrecoverable.
- Mitigation: not yet defined — a below-threshold recovery/succession
  procedure is an Open Decision, below.

No protocol-level enforcement at all:

- Risk: every mitigation in this document is weaker than an equivalent
  on-chain mechanism would be, precisely because none of it is verifiable
  or enforceable by consensus.
- Mitigation: acknowledged explicitly, not glossed over (§2) — this is a
  deliberate v1 scope choice with a named path to revisit it (a future
  dedicated ADR for on-chain threshold/multisignature and vesting
  primitives), not a permanent ceiling on what this project could do.

Key generation ceremony failure:

- Risk: a flawed ceremony (insufficient witnesses, a compromised entropy
  source, or a transcription error in the published address) produces an
  account the intended custodians do not actually control, or that a third
  party can also access.
- Mitigation: §7's witnessed, independently-recomputed-address procedure.

## 9. Compatibility

This document introduces no consensus rule change and no wire-format
change. A validator or light client cannot distinguish a transaction from
one of these three accounts from a transaction sent by any other ordinary
`account`-namespace address. Introducing a future on-chain
multisignature or vesting mechanism for these accounts (§2) would be a
protocol change requiring its own accepted ADR — it would not be a
revision to this document alone, the same bar ADR-0015 sets for activating
slashing and ADR-0024 sets for any change to its own Protocol Upgrade
Constraints list.

## 10. Open Decisions

- Liquidity & Ecosystem Reserve custody threshold (`M`-of-`N`)
- Founder Allocation custody threshold (`M`-of-`N`)
- Community & Airdrop Allocation custody threshold (`M`-of-`N`)
- Founder Allocation vesting schedule: cliff length, total duration,
  release curve (linear vs. milestone-based)
- Custodian selection policy per account (who, how many, organizational/
  geographic diversity requirements)
- Below-threshold recovery/succession procedure for a custodian who is
  lost, compromised, or becomes unavailable
- Key generation ceremony operational detail (specific tooling, minimum
  witness count, which of the two §4 patterns each account uses)
- Community & Airdrop Allocation's end-recipient distribution mechanism
  and schedule (ADR-0024's own separate Open Decision item — not resolved
  by this document)
- Liquidity & Ecosystem Reserve spending authorization / use-case approval
  process (ADR-0024's own separate Open Decision item — not resolved by
  this document)
- Whether and when to revisit on-chain threshold/multisignature and
  vesting enforcement via a dedicated future ADR (§2)

## Related Specifications

- `docs/adr/ADR-0024-hncoin-monetary-policy.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0003-address-format.md`
- `docs/specs/core/genesis.md`
- `docs/specs/core/account-state.md`
- `docs/security/README.md`
