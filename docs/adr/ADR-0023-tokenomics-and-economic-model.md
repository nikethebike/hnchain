# ADR-0023: Tokenomics And Economic Model

Status: Proposed

Date: 2026-09-11

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0009: Consensus Architecture
- ADR-0010: Validator Set Model
- ADR-0015: Slashing And Accountability

Supersedes: None

Referenced By:

- ADR-0024: HNCOIN Monetary Policy (resolves the "Monetary policy" and
  "Genesis allocation" areas below — a focused sub-ADR under this one's
  umbrella, the same relationship ADR-0011/ADR-0015 have to ADR-0009)

## Context

Every economic or monetary parameter in this project — genesis supply,
allocation, fee amounts, validator rewards, minimum bond, unbonding
period, slashing amounts, governance voting weight — has been
deliberately left undecided everywhere it was touched, each time
pointing to the same not-yet-existing document: "a future tokenomics
specification." That pointer appears in the whitepaper (Chapter XIII,
"Economic Model," which explicitly labels every concrete number it
mentions as "not yet accepted"), in ADR-0006 ("Decided: fee mechanism
only... amount, split, or market model... remain owned by a future
tokenomics specification"), in ADR-0010 ("minimum validator bond,"
`cap_numerator`/`cap_denominator`, unbonding period — all Open
Decisions), and in ADR-0015 ("Slashing is not activated by this ADR...
every item in this list stays blocked on a staking/delegation/tokenomics
track that does not exist anywhere in this project yet"). No such
document has existed until this ADR.

The whitepaper itself states the precondition for writing one: "The
economic model cannot be finalized before consensus" (§12.9, "Consensus
As Economic Core"). That precondition is now met — the consensus track
(ADR-0009 through ADR-0018) is closed end-to-end: consensus family
(Tendermint-style BFT), validator admission/jailing/active-set mechanics,
voting power model (capped stake-weighted, mechanism decided), fee
mechanism (cap-not-exact, sender-paid, still-incurred-on-failure), and
slashing's evidence/jailing framework are all mechanically decided.
What remains in every one of those ADRs, without exception, is the
*amount* or *policy value* — never the mechanism.

This ADR exists to be that single owning document, consolidating what
was scattered as identical "deferred to a future tokenomics
specification" placeholders across half a dozen other ADRs into one
place, decided the same one-decision-at-a-time way every other
multi-decision pass in this project has been (ADR-0003's address
format, ADR-0006's transaction format, the consensus track itself).

Every value this ADR eventually decides is subject to the project's own
standing constraint: no economic or monetary parameter — including an
explicit zero — may be encoded in implementation before being accepted
here. Values the whitepaper floated as illustrative and explicitly
**not accepted** (a 100,000,000 HNC maximum supply target, 0% annual
inflation) are superseded by ADR-0024's own different, now-accepted
numbers (a 1,000,000,000 HNCOIN maximum supply; 0% inflation confirmed,
but as a decided value, not an illustrative one). The whitepaper's other
still-unaccepted candidate, a 70%/30% validator/burn fee split, remains
exactly that — a candidate, not a decision — under this ADR's own Fees
area, below.

## Decision

HNChain's tokenomics and economic model are decided in this ADR,
incrementally, across the following areas — mirroring the whitepaper's
own Chapter XIII structure so every whitepaper-flagged open item has a
home here:

- **Monetary policy** — **Decided, ADR-0024**: fixed `1,000,000,000
  HNCOIN` maximum supply, created entirely at genesis, no post-genesis
  minting, 0% inflation, no halving (nothing to halve).
- **Genesis allocation** — **Decided, ADR-0024**: three genesis
  accounts — Liquidity & Ecosystem Reserve (80%), Founder (10%),
  Community & Airdrop (10%). Key material, multisig, vesting, and
  spending-authorization detail for those three accounts remain open
  (ADR-0024's own Open Decisions).
- **Fee economics**: the amount/market model on top of ADR-0006's
  already-decided mechanism — base fee or congestion pricing, priority
  fees, minimum fee floor, burn ratio, validator distribution, storage
  fees/rent.
- **Staking economics**: minimum validator bond, unbonding period,
  `MAX_ACTIVE_SET_SIZE`, voting-power cap ratio (`cap_numerator`/
  `cap_denominator`), delegation model and stake concentration limits
  (if any), validator reward mechanism and distribution.
- **Slashing economics**: activation criteria, penalty amounts (fixed
  vs. proportional), jail duration, delegator impact, evidence fees.
- **Treasury and development funding**: mechanism (if any), funding
  source, governance oversight.
- **Governance economic weight**: voting model (explicitly not
  `1 HNC = 1 vote` by default, per the whitepaper's own rejection of
  that as a default principle).

No value in any of these areas was decided by this ADR's initial
version; monetary policy and genesis allocation are now decided, via
ADR-0024, a focused sub-ADR rather than an edit to this document
(mirroring ADR-0009's own relationship to ADR-0010/ADR-0011/ADR-0015).
The remaining five areas are still open. Status stays `Proposed` until
a meaningful subset is directly accepted here; unlike most other ADRs
in this project, this one is expected to be amended repeatedly over
many sessions as each area is worked, the same way ADR-0006/ADR-0009/
ADR-0010 were each built up decision by decision rather than written
complete on day one.

## Normative Rules

### No Implicit Economic Defaults

No economic or monetary parameter (genesis supply, allocation, fee
amount, burn percentage, reward amount, minimum bond, unbonding period,
slashing amount, or any other value in this ADR's scope) may be encoded
in consensus-critical implementation, test fixtures used as if they were
canonical, or genesis construction, until it is accepted in this ADR.
This includes an explicit zero — a "0 HNC founder allocation" is as much
a policy decision requiring deliberate acceptance as any nonzero value.

### One Owning Document

Once this ADR accepts a value for a parameter previously listed as an
Open Decision in another ADR (ADR-0006, ADR-0010, ADR-0015, or any
other), that other ADR's own Open Decisions entry is updated to
cross-reference this one rather than restating the value — the same
single-source-of-truth discipline `consensus_root`/`validator_set_commitment`
already established for validator-set state (ADR-0010).

### Consensus Precedes Economics, Not The Reverse

No decision in this ADR may require reopening an already-Accepted or
already-decided consensus mechanism (family, voting power computation,
finality rule, fork-choice rule) to make economic sense. If a proposed
economic parameter would require such a reopening, that is a sign the
parameter itself needs reconsidering, not the consensus mechanism —
matching the whitepaper's own stated ordering (§12.9).

## Rejected Options

### Deciding Economic Parameters Ad Hoc, Inside Each Owning ADR

Rejected because it already produced exactly the scattering problem
this ADR exists to fix: the same "minimum bond," "unbonding period," and
"cap ratio" placeholders independently duplicated across ADR-0006,
ADR-0010, and ADR-0015's own text, each pointing to a document that did
not exist. A future reader diffing those three ADRs against each other
has no way to tell whether they still agree without this consolidation.

### Deferring This ADR Until After DEVNET

Rejected because every other cross-cutting ADR pass in this project's
history (address format, transaction format, consensus family) was
written well ahead of the implementation phase that needed it, on the
same spec-first principle this project holds everywhere else — waiting
until code is blocked on a missing number is the failure mode this
project's whole workflow (architecture → specifications → review →
implementation) exists to avoid.

### Picking The Whitepaper's Candidate Numbers By Default

Rejected because the whitepaper explicitly and repeatedly marks every
number it states (100,000,000 HNC, the 70/30 fee split, 0% inflation) as
illustrative, not accepted, and explicitly requires "distribution
modeling, validator reward simulation, fee-market analysis, and
ecosystem requirements" before any of them could be. Treating an
unaccepted illustrative number as a default would be exactly the
implicit-parameter risk [[hnchain-economic-decision-constraint]] exists
to prevent.

## Alternatives Considered

### Fixed Maximum Supply, No Continuing Emission (Selected — ADR-0024)

Advantages:

- simple, predictable monetary policy
- avoids continuous dilution of existing holders

Disadvantages:

- long-term validator security budget depends entirely on fees once
  genesis allocation is exhausted — resolved by ADR-0024's own
  decision to fund validator rewards from transaction fees, not
  emission
- no emission-funded treasury or ongoing security subsidy

### Continuing Emission (Inflationary Issuance) (Rejected — ADR-0024)

Advantages:

- funds validator rewards and/or treasury independent of fee revenue
- can be tuned over time via governance if a schedule allows it

Disadvantages:

- dilutes existing holders unless offset by burn or utility growth
- requires an accepted emission curve, which does not exist yet

### No Protocol Treasury

Advantages:

- avoids governance-capture risk that comes with a centrally
  controlled fund
- simplest monetary policy

Disadvantages:

- critical infrastructure (audits, core development) may be
  underfunded without an external funding source

## Security Considerations

Validator security budget:

- Risk: a fixed supply with no emission, combined with fees too low to
  fund security, leaves validators economically unable to secure the
  network long-term.
- Mitigation: ADR-0024 already resolves the *mechanism* (fee-funded,
  never emission-funded); economic modeling of the fee *amount* itself
  is still required before this ADR's own Fees area is accepted.

Wealth-concentration governance capture:

- Risk: `1 HNC = 1 vote` or similar direct-plutocratic models let
  concentrated holdings buy protocol control.
- Mitigation: this ADR rejects `1 HNC = 1 vote` as a default; a
  separate governance-weight decision is required.

Premature slashing amounts:

- Risk: accepting slashing amounts before staking/delegation/evidence
  rules are all in place risks mass over-penalization from a client
  bug or ambiguous rule, exactly the "mass slashing" risk ADR-0015
  already flags.
- Mitigation: this ADR's own decisions in the slashing-economics area
  must respect ADR-0015's own listed preconditions before activation.

Founder/insider allocation optics and centralization:

- Risk: a genesis allocation perceived as unfair or centralizing
  undermines legitimacy regardless of technical correctness.
- Mitigation: transparent, publicly documented allocation and vesting
  terms, decided here rather than left implicit.

## Compatibility

Accepting a value for a parameter in this ADR that a live network has
already launched with (post-genesis) is a major protocol change and
requires activation and compatibility review, the same bar ADR-0015
already sets for activating slashing. Accepting a value before genesis
(the expected case for most of this ADR's scope) is not a compatibility
event — it defines genesis, rather than changing it.

## Open Decisions

Genesis supply and allocation — **resolved, see ADR-0024** (maximum
supply, the three allocation amounts, no-post-genesis-mint, 0%
inflation, no halving, and the fee-funded-rewards security-budget
mechanism are all decided there). What ADR-0024 itself leaves open:
genesis allocation account key material/multisig/vesting, the
Community & Airdrop distribution schedule, and Liquidity & Ecosystem
Reserve spending authorization — see ADR-0024's own Open Decisions.

Fees (mechanism decided, ADR-0006 — amounts and policy open here):

- base fee or congestion pricing model
- priority fee model, if any
- minimum fee floor
- fee burn ratio (if any) and validator distribution split
- storage fees / rent policy (structural support decided,
  account-state.md §Metadata/Rent; policy itself open)
- resource metering formula (gated jointly on a future HNVM metering
  specification, not owned solely by this ADR)

Staking and validator economics (mechanism decided, ADR-0010):

- `MAX_ACTIVE_SET_SIZE` (the cap value `K`)
- `cap_numerator` / `cap_denominator` (voting-power capping ratio)
- voting power maximum value bound (if any, below `u128::MAX`) and
  zero-power behavior
- minimum validator bond
- unbonding period
- epoch length (`EPOCH_LENGTH`)
- delegation support and design, if any; stake concentration limits
- validator reward mechanism and distribution (no reward mechanism of
  any kind exists yet in this project)

Slashing economics (evidence/jailing mechanism decided, ADR-0015):

- slashing activation criteria
- slashing amounts (fixed vs. proportional)
- jail duration / release condition (constant only — mechanism decided)
- delegator impact model
- evidence fees
- correlated-failure policy
- downtime penalty policy (if any — evidence-based equivocation
  jailing is already decided and does not depend on this)

Treasury and development funding — distinct from ADR-0024's Liquidity &
Ecosystem Reserve (an ordinary funded account, decided) and from the
already-decided but still-unfunded `treasury` protocol object
(ADR-0007, `system` domain `0x0009`, keyless):

- mechanism (genesis allocation with vesting, ongoing fee share,
  grants, ecosystem fund, or none)
- funding source and governance oversight, if a treasury is accepted

Governance economic weight:

- voting model (delegated, reputation-weighted, quadratic, chambers,
  time-locked, technical council, or a combination) — `1 HNC = 1 vote`
  is already rejected as the default (Decision, above)
- economic control limits on governance itself

Other:

- protocol/bridged asset definitions (supply, decimals, mint/burn
  authority) for any curated non-native asset (structural placement
  decided, ADR-0007's `assets` domain — no concrete asset defined yet)
- bridge reserve handling

## Related Specifications

- `docs/whitepaper/HNChain-Whitepaper-v0.1-draft.md` (Chapter XIII,
  "Economic Model")
- `docs/adr/ADR-0024-hncoin-monetary-policy.md`
