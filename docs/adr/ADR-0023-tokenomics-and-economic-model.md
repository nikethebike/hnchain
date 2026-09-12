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
- ADR-0025: Governance Model (resolves the "proposal process" part of
  "Governance economic weight" below, the same focused-sub-ADR
  relationship)

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
- **Fee economics** — **partially decided**: fixed-rate fee model for
  `tx_version = 1` (no congestion pricing, no priority fee); 70%
  validator / 30% burn distribution split. Fee floor/rate amount and
  storage fees/rent stay open.
- **Staking economics** — **partially decided**: `MAX_ACTIVE_SET_SIZE
  = 100`; `cap_numerator/cap_denominator = 1/10`; unbonding period = 21
  days; `EPOCH_LENGTH = 43,200` blocks (24 hours); delegation supported
  from genesis; validator reward = 70% of transaction fees (the same
  figure as the fee split above — one parameter, not two). Minimum
  validator bond, stake concentration limits, and key rotation delay
  stay open.
- **Slashing economics** — **partially decided**: monetary slashing
  stays not activated (jailing, ADR-0015, remains the only active
  accountability mechanism); evidence submission has no fee. Slashing
  activation criteria, penalty amounts, delegator impact, and
  correlated-failure policy stay open — all gated on activation, which
  has not happened.
- **Treasury and development funding** — **decided: none.** The
  Liquidity & Ecosystem Reserve (ADR-0024) covers this role; no
  separate protocol-owned treasury is funded.
- **Governance economic weight** — **decided: validator + staker
  chambers.** Validators and stakers vote as two separate bodies whose
  agreement is both required — not `1 HNC = 1 vote`, and not a single
  undifferentiated token-weighted pool. Proposal process decided
  separately (ADR-0025, Governance Model — signaling-only, `Active`-
  validator proposers, quorum-then-majority per chamber). Quorum
  percentage, voting window length, and scope of what governance may
  decide stay open.

Each resolved item's full normative content is under "Decided," below,
grouped by area. Status stays `Proposed` — several items above remain
genuinely open (mostly amounts requiring dedicated economic modeling,
per the whitepaper's own repeated caution), and this ADR is expected to
be amended further as they are worked, the same incremental pattern
ADR-0006/ADR-0009/ADR-0010 each went through before reaching
`Accepted`.

### Decided: Active Set Size And Voting Power Cap

```text
MAX_ACTIVE_SET_SIZE (K) = 100
cap_numerator / cap_denominator = 1 / 10
```

Plugs directly into ADR-0010's already-decided mechanisms: `active_set`
selects the top 100 `Active` validators by `voting_power` descending
(`ranked.take(MAX_ACTIVE_SET_SIZE)`); the capping algorithm bounds any
single validator's per-round voting power to at most 10% of that
round's total (`C_r = floor(total_r / 10)`) before ranking. Neither
formula changes — only their previously-parameterized inputs are now
fixed. Not yet wired into code: the capping algorithm itself
(`C_r = floor(cap_numerator * total_r / cap_denominator)`, recomputed
over the whole candidate set) has no implementation yet, only the
already-implemented top-K selection in
[`hn_state::active_set`](../../hn-state/src/active_set.rs) does.

### Decided: Delegation Supported

Delegated staking is supported from genesis — not deferred to a later
protocol version. Stake concentration limits (a cap on how much of one
validator's `voting_power` may come from delegators, or how
concentrated delegation may be toward one validator) remain open: no
dominant industry practice exists to derive a starting value from, and
the underlying delegation transaction/reward-accounting design itself
is not yet specified (ADR-0006 lists `stake`/`unstake` as sender-only,
with no delegator-vs-self-stake distinction yet — a real, separate
design task this decision does not resolve).

### Decided: Unbonding Period

```text
UNBONDING_PERIOD = 21 days
UNBONDING_PERIOD_BLOCKS = 907_200
```

A wall-clock duration, not an epoch count — consistent with ADR-0010's
own framing ("real-world unbonding periods measured in weeks, not one
epoch"). Applies from the moment `unstake` reduces `bonded_stake`
(ADR-0006) until the withdrawn amount becomes actually spendable.
Expressed in blocks for the actual mechanism (`21 * 24 * 60 * 60 / 2`)
using ADR-0009's own "Decided: Target Block Time" (`2` seconds) —
decided alongside this item specifically because implementing the
release mechanism needed it: `BlockHeader.timestamp`'s own consensus
semantics remain undecided (ADR-0008, "Timestamp"), so a
consensus-critical maturity check cannot yet use wall-clock time
directly, the same reasoning `ValidityWindowV1` (ADR-0006) already
applied to a similar problem.

The release mechanism is implemented, not left as a future task: `unstake`
([`hn_state::apply_unstake`](../../hn-state/src/validator_transition.rs))
records a `PendingUnbondingV1 { amount, matures_at_height }` on the
`ValidatorRecordV1` rather than crediting the account immediately;
[`hn_state::apply_unbonding_release`](../../hn-state/src/validator_transition.rs)
credits it back to the account's native balance once `matures_at_height`
is reached, clearing the pending record. At most one pending withdrawal
per validator is supported (a second `unstake` while one is pending is
rejected, `StateError::PendingUnbondingAlreadyExists`) — the simplest
correct behavior for a first implementation, not a queue; a bounded
queue of several simultaneous withdrawals remains a natural, additive
future generalization if ever needed. Nothing in this codebase calls
`apply_unbonding_release` yet — it is a state-transition primitive
waiting for a block-processing pipeline (`hn-consensus`/`hn-node` are
still stubs) to invoke it as a periodic sweep, the same "mechanism
implemented, integration point does not exist yet" situation
`hn_state::active_set`'s own capping algorithm is in.

### Decided: Epoch Length

```text
EPOCH_LENGTH = 43_200 blocks
```

24 hours, using ADR-0009's own `TARGET_BLOCK_TIME` (2 seconds) — the
same conversion `UNBONDING_PERIOD_BLOCKS` already used, and the same
reason `EPOCH_LENGTH` itself needed `TARGET_BLOCK_TIME` decided first
(ADR-0010's "Epoch Boundaries" already fixed epoch boundaries as
height-aligned; only the constant was missing). Chosen over shorter
candidates (1 hour, 6 hours): a full day keeps checkpoint/light-client
tracking overhead low and gives operators an easily-communicated
admission delay, while staying clearly shorter than the 21-day
unbonding period above — the two govern different concerns (how often
the active set updates vs. how long withdrawn stake is held) and
should not be easy to confuse by being close in magnitude. Full detail
recorded in ADR-0010's own "Epoch Boundaries" section, which owns the
mechanism this value plugs into; not duplicated here beyond this
pointer, per this ADR's own "One Owning Document" rule.

No code currently consumes this value: nothing in `hn-state` yet
converts a height into an epoch number (`ConsensusVote`/
`QuorumCertificate` carry `epoch` as an explicit field today, not
derived from height) — the same "decided, not yet a consumer" state
`MAX_ACTIVE_SET_SIZE`/`cap_numerator` were already in before this
pass, not a gap specific to this value.

### Decided: Minimum Validator Bond — Still Open, Reasoning Recorded

Deliberately not fixed to an absolute HNCOIN figure: without a stable
market price estimate for HNCOIN, an absolute figure risks being
either meaningless (too low, in real terms, after any price
appreciation) or prohibitive (too high, if HNCOIN's value is initially
uncertain) — the same accessibility/Sybil-resistance/decentralization
tension the whitepaper itself already flagged (§12.4) as one of the
most sensitive parameters in the whole model. A future revision of this
item may denominate it some other way (for example, a share of total
supply, or fiat-pegged with a defined re-pricing rule) rather than a
fixed atomic-unit constant — that mechanism question is itself still
open, not just the number.

### Decided: Validator Reward / Fee Distribution

```text
validator_share = 70%
burn_share       = 30%
```

Applies to collected transaction fees (ADR-0006's already-decided
cap-not-exact, sender-paid mechanism) — the same figure resolves two
Open Decision items at once ("validator reward mechanism and
distribution," Staking; "fee burn ratio... and validator distribution
split," Fees), since a fee's only two destinations are validators and
burn: no protocol treasury exists to take a third share (see "Decided:
No Protocol Treasury," below). This is the whitepaper's own §12.3
candidate split, now accepted rather than illustrative. Validator
rewards are funded entirely from this share of existing, collected
fees, per ADR-0024's already-decided "Validator Rewards And Transaction
Fees" — never newly minted HNCOIN. The fee *amount* itself (what gets
collected before this split applies) remains open, below.

### Decided: Fee Model For `tx_version = 1`

`tx_version = 1` uses a fixed-rate fee, not congestion pricing (no
EIP-1559-style dynamic base fee) and no priority fee mechanism — the
simplest model consistent with ADR-0006's already-decided cap-not-exact
mechanism. A future `tx_version` may introduce congestion pricing or
priority fees without this ADR needing to be reopened, the same
Structure Versioning discipline (ADR-0022) already applies everywhere
else in this project. The actual fixed rate, and the minimum fee floor,
remain open — genuinely amount-level decisions requiring the same
economic modeling the whitepaper's own Fee Burning section (§12.3)
already calls for, not decided by picking the model alone.

### Decided: Storage Fees / Rent — Not Activated

No change from account-state.md's own already-existing position ("A
rent policy is not activated by this specification... the account
model must preserve enough structure to introduce rent... without
changing the meaning of existing account fields"). This ADR does not
activate it either; restated here only so this area's own Open
Decisions entry does not look silently unaddressed.

### Decided: Slashing Stays Not Activated

Monetary slashing is not activated by this decision. Jailing (ADR-0015,
already active) remains the only live accountability mechanism.
Evidence submission carries no fee — free submission avoids
discouraging legitimate evidence, the common practice across BFT
networks this project's own Tendermint-family choice (ADR-0009) draws
from; a fee would only meaningfully deter spam if set high enough to
also deter honest, occasional reporting, which is the wrong trade for a
mechanism whose entire value depends on evidence actually being
submitted. Slashing activation criteria, penalty amounts, delegator
impact, and correlated-failure policy stay open, gated on activation —
see ADR-0015's own "Slashing" section for the full precondition list
this decision does not shorten.

### Decided: No Protocol Treasury

No separate protocol-owned treasury is funded. The Liquidity &
Ecosystem Reserve (ADR-0024, 800,000,000 HNCOIN, an ordinary
key-controlled account) already covers the ecosystem-development-
funding role the whitepaper's own §12.6 "Development Funding" section
described a treasury as one candidate mechanism for — one of that
section's own listed candidates was explicitly "no protocol treasury,
relying only on external funding," and that is the option selected
here, satisfied by the Reserve rather than by nothing. The already-
decided `treasury` protocol object (ADR-0007, `system` domain `0x0009`,
keyless) is unaffected by this decision: it remains structurally
reserved but is not funded by this ADR or any other, and stays
available for a future governance-driven funding decision if one is
ever made — this ADR does not foreclose that, it just does not fund it
now.

### Decided: Governance Voting Model

Validators and stakers vote as two separate chambers; a governance
proposal requires agreement from both, not a single pooled
token-weight count. This resolves the whitepaper's own explicit
rejection of `1 HNC = 1 vote` as a default (§12.7) with one of its own
named candidate models ("validator and staker chambers") rather than
inventing a new one. Chosen over delegated voting (needs its own
delegation infrastructure, distinct from staking delegation) and a
technical council (too centralized for a starting model) for this
first decision.

**Proposal process resolved separately, ADR-0025 (Governance
Model)**: signaling-only proposals (no automatic on-chain effect),
`Active` validators may propose, per-chamber quorum-then-majority pass
rule, height-based voting window. Left open there and here: the
quorum percentage and voting window length themselves (tunable
economic parameters, this ADR's own scope), what governance is
actually empowered to decide (a distinct, larger question ADR-0025
explicitly defers, not resolved by deciding signaling-only), and
whether staking delegation (decided above) also carries delegated
governance voting weight — blocked on delegation's own tracking
mechanism not existing yet, the same gap ADR-0025 itself names.

### Decided: HNCOIN Decimals And Atomic Unit

```text
DECIMALS = 9
ATOMIC_UNIT = hnit
```

Resolves the item ADR-0024 itself left open ("HNCOIN's atomic unit /
decimal convention for `native_balance: u128`"). `native_balance`
(ADR-0001) counts `hnit`s, not whole HNCOIN: `1 HNCOIN = 1,000,000,000
hnit`. `GENESIS_SUPPLY` in atomic units is therefore
`1,000,000,000 * 10^9 = 10^18 hnit` — far below `u128::MAX`
(~3.4 × 10^38), leaving enormous headroom for the same
intermediate-computation overflow concerns account-state.md's own
`native_balance` documentation already anticipated (fee/reward
multiplication and similar). 9 decimals is the same width Solana uses
for its own smallest SOL unit (lamports) — not adopted because it is
Solana's choice, but noted as an already-proven width for a
high-throughput chain's native balance granularity.

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
to prevent. The 70/30 split was later accepted anyway ("Decided:
Validator Reward / Fee Distribution," above) — the point is that it was
decided deliberately, through this ADR's own process, not carried over
automatically because the whitepaper happened to mention it first.

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

### No Protocol Treasury (Selected)

Advantages:

- avoids governance-capture risk that comes with a centrally
  controlled fund
- simplest monetary policy
- the Liquidity & Ecosystem Reserve (ADR-0024) already exists to cover
  ecosystem/development funding, so this is not actually "no funding,"
  only "no separate protocol-owned treasury object"

Disadvantages:

- critical infrastructure (audits, core development) may be
  underfunded without an external funding source, if the Reserve's own
  spending authorization (ADR-0024, still open) turns out too
  restrictive in practice

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
- Mitigation: "Decided: Governance Voting Model," above — validator +
  staker chambers, requiring both bodies' agreement, rather than a
  single pooled token-weighted vote.

Premature slashing amounts:

- Risk: accepting slashing amounts before staking/delegation/evidence
  rules are all in place risks mass over-penalization from a client
  bug or ambiguous rule, exactly the "mass slashing" risk ADR-0015
  already flags.
- Mitigation: "Decided: Slashing Stays Not Activated," above — this
  batch pass deliberately resolved several slashing preconditions
  (delegation, unbonding period) without activating slashing itself,
  precisely to avoid this risk.

Founder/insider allocation optics and centralization:

- Risk: a genesis allocation perceived as unfair or centralizing
  undermines legitimacy regardless of technical correctness.
- Mitigation: transparent, publicly documented allocation and vesting
  terms, decided here rather than left implicit.

Unbounded delegation concentration:

- Risk: delegation is decided (supported from genesis) without stake
  concentration limits — one validator could in principle accumulate
  delegated stake well beyond what the 10% per-round voting-power cap
  alone constrains at the raw `bonded_stake` level, even though
  `voting_power` itself stays capped.
- Mitigation: the 10% voting-power cap (`cap_numerator/cap_denominator
  = 1/10`) already bounds the consequence that matters for consensus
  safety regardless of how concentrated delegation gets; concentration
  limits, if added later, would be a defense-in-depth addition, not a
  safety requirement this ADR leaves unmet.

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
inflation, no halving, decimals/atomic unit, and the fee-funded-rewards
security-budget mechanism are all decided there or above). What
ADR-0024 itself leaves open: genesis allocation account key
material/multisig/vesting, the Community & Airdrop distribution
schedule, and Liquidity & Ecosystem Reserve spending authorization —
see ADR-0024's own Open Decisions.

Fees (mechanism decided, ADR-0006; model decided above — fixed-rate,
70/30 validator/burn split; amounts still open):

- the fixed fee rate itself
- minimum fee floor
- storage fees / rent policy (structural support decided,
  account-state.md §Metadata/Rent; not activated — decided above)
- resource metering formula (gated jointly on a future HNVM metering
  specification, not owned solely by this ADR)

Staking and validator economics (mechanism decided, ADR-0010;
`MAX_ACTIVE_SET_SIZE`, cap ratio, unbonding period, `EPOCH_LENGTH`,
delegation support, and validator reward share all decided above):

- voting power maximum value bound (if any, below `u128::MAX`) and
  zero-power behavior
- minimum validator bond (reasoning for leaving it open recorded
  above, under "Decided: Minimum Validator Bond")
- stake concentration limits (delegation itself is decided — supported
  — only a concentration cap, if any, remains open)
- key rotation delay

Slashing economics (evidence/jailing mechanism decided, ADR-0015;
activation stays not-activated and evidence fees are decided — none —
per "Decided: Slashing Stays Not Activated," above):

- slashing activation criteria (blocked on activation itself, not
  independently open)
- slashing amounts (fixed vs. proportional) — same block
- delegator impact model — same block
- correlated-failure policy — same block
- downtime penalty policy (if any — evidence-based equivocation
  jailing is already decided and does not depend on this)

Treasury and development funding — **resolved: no protocol treasury**
("Decided: No Protocol Treasury," above). Nothing left open in this
area.

Governance economic weight — voting model **decided: validator +
staker chambers** (above); proposal process **decided, ADR-0025**
(signaling-only, `Active`-validator proposers, quorum-then-majority per
chamber, height-based voting window — mechanisms only); left open:

- quorum percentage per chamber (ADR-0025's own mechanism, this ADR's
  value)
- `GOVERNANCE_VOTING_WINDOW` length (same split)
- scope of what governance may decide
- whether staking delegation also carries delegated governance voting
  weight, or needs its own separate delegation step (blocked on
  delegation's own tracking mechanism, ADR-0025)

Other:

- protocol/bridged asset definitions (supply, decimals, mint/burn
  authority) for any curated non-native asset (structural placement
  decided, ADR-0007's `assets` domain — no concrete asset defined yet;
  distinct from HNCOIN's own now-decided decimals, above, which apply
  only to the native balance)
- bridge reserve handling

## Related Specifications

- `docs/whitepaper/HNChain-Whitepaper-v0.1-draft.md` (Chapter XIII,
  "Economic Model")
- `docs/adr/ADR-0024-hncoin-monetary-policy.md`
- `docs/adr/ADR-0025-governance-model.md`
