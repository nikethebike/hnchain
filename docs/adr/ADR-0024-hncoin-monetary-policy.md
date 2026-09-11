# ADR-0024: HNCOIN Monetary Policy

Status: Proposed

Date: 2026-09-11

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0003: Address Format
- ADR-0006: Transaction Format
- ADR-0007: State Tree
- ADR-0023: Tokenomics And Economic Model

Supersedes: None

## Context

HNCOIN is HNChain's native asset (ADR-0001, "Balance" — the singleton
`native_balance: u128` field every account has, distinct from the
`assets` domain's variable-cardinality collection of non-native,
protocol-curated holdings, ADR-0007). Its monetary policy — total
supply, how it comes into existence, whether it can be created after
genesis, and how it may be destroyed — has been an open item since the
balance/asset split was first decided, deferred every time to "a future
tokenomics specification" that did not exist until ADR-0023.

ADR-0023 (Tokenomics And Economic Model) is the umbrella document
created to own every scattered economic Open Decision in this project.
This ADR fills in the specific sub-areas ADR-0023 lists as "Monetary
policy" and "Genesis allocation" — the same relationship ADR-0010
(Validator Set Model)/ADR-0011 (Leader Election)/ADR-0015 (Slashing And
Accountability) already have to ADR-0009 (Consensus Architecture): a
focused, single-topic ADR under a broader umbrella, not a competing or
duplicate owner. ADR-0023's own text is updated alongside this one to
mark these two areas resolved and point here instead of restating the
values.

The whitepaper (Chapter XIII, "Monetary Policy") floated an illustrative
`100,000,000 HNC` maximum supply and explicitly labeled it "not yet an
accepted monetary policy... an economic design parameter, not a magic
constant." This ADR supersedes that illustrative number with a
different, now-accepted one; the whitepaper itself stays as historical/
non-binding context per this project's own Status Rules and is not
edited by this ADR.

This ADR models HNCOIN on Bitcoin's own monetary discipline — a fixed
supply created once, no ongoing issuance, no halving schedule (because
there is no emission to halve) — while resolving the specific security
concern the whitepaper itself already raised about that model (§12.8:
"the network must eventually rely on fees, treasury policy, or another
explicitly defined mechanism to fund security" once there is no
continuing emission). "Validator Rewards And Transaction Fees," below,
resolves that concern the same way the whitepaper's own text
anticipated: validator rewards, if and when enabled, are funded from
transaction fees — existing HNCOIN moving between accounts, never newly
created HNCOIN.

## Decision

HNCOIN uses a fixed-supply, non-inflationary monetary model. The entire
maximum supply is created exactly once, at genesis.

```text
MAX_SUPPLY         = 1,000,000,000 HNCOIN
GENESIS_SUPPLY      = 1,000,000,000 HNCOIN
POST_GENESIS_MINT   = FORBIDDEN
INFLATION            = 0%
HALVING              = NOT APPLICABLE
```

After genesis, HNChain has no protocol mechanism capable of creating
additional HNCOIN. Supply may only remain unchanged (transfers) or
decrease (protocol-defined burn).

### Genesis Allocation

The genesis supply is allocated into three categories, each an ordinary
`account`-namespace account (ADR-0003) seeded with a balance at genesis
— not a `protocol`-namespace system object (see "Consistency With
ADR-0003" below):

| Allocation                     | Amount              | Share |
|---------------------------------|--------------------|-------|
| Liquidity & Ecosystem Reserve   | 800,000,000 HNCOIN | 80%   |
| Founder Allocation              | 100,000,000 HNCOIN | 10%   |
| Community & Airdrop Allocation  | 100,000,000 HNCOIN | 10%   |
| **Total**                       | **1,000,000,000 HNCOIN** | **100%** |

No additional monetary allocation exists outside this table. Access
control, multisignature configuration, vesting, and timelocks for these
three accounts are explicitly deferred to a future genesis-security
specification (Open Decisions, below) — this ADR fixes the *amounts*
and the fact that they are genesis-only, not *who holds the keys* or
*under what spending restrictions*.

### Consistency With ADR-0003 (Address Format)

ADR-0003's `protocol` namespace is a **closed registry** for
`address_version = 1`: `treasury` (`0x01`), `governance` (`0x02`),
`staking` (`0x03`), `slashing` (`0x04`), `bridge registry` (`0x05`) —
each a genesis-assigned, keyless, `31 zero bytes || module_id` address
with no controlling key at all (ADR-0003, "Protocol Namespace
Addresses"). Extending that registry needs a governance ADR that does
not exist yet.

The three genesis allocations this ADR defines are **not** additions to
that registry and do not need one: they are ordinary, key-controlled
`account`-namespace addresses, pre-funded at genesis, exactly like any
other account created with an initial balance. This also means
**"Liquidity & Ecosystem Reserve" is a distinct concept from the
already-decided `treasury` protocol object** (ADR-0007, `system` domain
`0x0009`) — the two are easy to conflate by name, but `treasury` is
keyless/protocol-owned and still fully unfunded (whitepaper §12.6,
"Development Funding," still an ADR-0023 Open Decision: no mechanism is
accepted for funding it), while the Reserve defined here is a normal
account holding real genesis-allocated HNCOIN from day one.

### Fixed Supply

```text
MAX_SUPPLY = 1,000,000,000 HNCOIN
GENESIS_SUPPLY = 1,000,000,000 HNCOIN
GENESIS_SUPPLY == MAX_SUPPLY
```

No unissued monetary supply remains available for future protocol
minting. The protocol must not contain a generic post-genesis operation
equivalent to `mint(amount)` for HNCOIN — no such `tx_type` exists in
ADR-0006's already-closed `tx_type` registry, and none may be added
while this policy stands.

### Supply Invariant

At every valid HNChain state:

```text
CURRENT_SUPPLY <= 1,000,000,000 HNCOIN
```

Before any burn, `CURRENT_SUPPLY = 1,000,000,000`. After a burn,
`CURRENT_SUPPLY < 1,000,000,000`. The transition
`CURRENT_SUPPLY(t+1) > CURRENT_SUPPLY(t)` is forbidden under this
policy; only a future ADR explicitly amending this one could permit it.

### No Post-Genesis Minting

After genesis, `mint(HNCOIN)` is invalid for every protocol actor:
validators, governance, treasury, founder, system accounts, smart
contracts, RPC/administrative interfaces, network operators. No
privileged account may bypass the fixed-supply invariant, and no future
protocol upgrade may introduce hidden issuance while claiming
compatibility with this policy.

### Validator Rewards And Transaction Fees

HNChain does not create new HNCOIN as validator rewards. Validator
rewards, when enabled (mechanism and amounts remain owned by ADR-0023
and ADR-0010, not decided here), must be funded from existing HNCOIN —
transaction fees are the preferred source. A transaction fee is a
transfer of existing economic value between accounts (ADR-0006's
already-decided fee mechanism: sender-paid, cap-not-exact), never an
issuance event: `fee ≠ mint`. Collecting a fee must not create new
HNCOIN; the exact distribution (validator share, burn share, any
treasury share) stays an ADR-0023 Open Decision.

### Burn Mechanism (Policy Decided, Wire Mechanism Deferred)

HNChain may support native protocol-level burning of HNCOIN. This ADR
decides the *invariant* burn must satisfy if and when it is
implemented, not a concrete `tx_type` or wire encoding — no `burn`
`tx_type` exists in ADR-0006's registry today, and adding one (or
folding burn into an existing operation's effects) is separate,
deferred work this ADR does not perform.

A burn is a protocol-level state transition, never modeled as an
ordinary transfer to a conventional user-controlled address — a
"burn address" pattern would leave the destroyed HNCOIN nominally
spendable by whoever controls that address's key, which does not
satisfy "permanently removes HNCOIN from the spendable monetary
supply." For every valid burn:

```text
burn_amount > 0
new_total_supply = previous_total_supply - burn_amount
```

A burn must never create HNCOIN, increase any balance, increase total
supply, be reversible, or return burned HNCOIN to circulation. A
zero-amount "burn" is a no-op and must not be represented as a
consensus state transition.

### No Halving

HNCOIN does not use a Bitcoin-style halving schedule. Halving exists to
reduce the rate new currency enters circulation under continuing
emission; HNCOIN has no post-genesis issuance to reduce, so there is no
`block_reward` to halve. `HALVING = NOT APPLICABLE` is a consequence of
the fixed-supply decision above, not an independent choice.

### Supply Accounting

```text
CURRENT_SUPPLY(t) = GENESIS_SUPPLY - TOTAL_BURNED(t)
                   = 1,000,000,000 - TOTAL_BURNED(t)
```

Ordinary transfers between accounts never change `CURRENT_SUPPLY` (a
debit and an equal credit net to zero); only a burn changes it, and
only downward. An independent node implementation must be able to
determine genesis supply, each of the three genesis allocations,
cumulative burned amount, and current total supply without a trusted
external database — this ADR does not mandate a specific accounting
mechanism (an explicit running counter vs. reconstructing supply by
summing every account balance is an implementation-specification
choice, naturally a candidate for a future `system`-domain singleton
alongside `treasury`, ADR-0007 — not decided here).

## Normative Rules

### Genesis Allocation Completeness

Genesis must explicitly define all three allocation accounts. Genesis
must not contain an implicit or undisclosed HNCOIN balance — every
genesis HNCOIN unit belongs to exactly one of the three allocations, and
their sum must equal `GENESIS_SUPPLY` exactly:

```text
100,000,000 + 800,000,000 + 100,000,000 = 1,000,000,000
```

Genesis construction must reject any configuration where
`allocation_sum != 1,000,000,000`.

### Forbidden Economic Shortcuts

The following implementation patterns are forbidden regardless of which
subsystem attempts them: `mint_for_validator`, `mint_for_treasury`,
`mint_for_governance`, `mint_for_founder`, `mint_for_emergency`,
`mint_for_upgrade`. No emergency-signaling mechanism may mint HNCOIN —
emergency authority may signal, it may not create currency. Governance,
validators, the founder allocation, and any treasury are account
holders, never monetary issuers.

### Monetary Policy Is Separate From Consensus

HNCOIN monetary policy stays separate from the consensus mechanism
(ADR-0009). Consensus enforces monetary policy (validates that no state
transition violates the supply invariant); it does not define monetary
policy. HN-PoS-style validator participation does not require
inflationary rewards, and this ADR's fixed-supply/fee-funded model does
not require or assume any particular consensus family beyond what
ADR-0009 already decided.

### Application-Level Usage Cannot Grant Minting Authority

HNCOIN may be consumed by ecosystem applications (HNMarket and similar)
through ordinary HNChain transactions. Application-level logic must
never gain arbitrary minting authority over HNCOIN — this ADR's
invariants bind at the protocol layer and cannot be loosened by any
higher application layer.

### Protocol Upgrade Constraints

Changing any of the following is a monetary policy change, not an
ordinary implementation change, and requires an explicit protocol-level
amendment (a new accepted ADR superseding or amending this one):
maximum supply, genesis supply, any genesis allocation amount,
post-genesis issuance, inflation, validator monetary issuance, burn
semantics, supply accounting, or monetary authority.

## Rejected Options

### Bitcoin-Style Halving Emission

Rejected because halving exists specifically to taper continuing
issuance; HNCOIN has no post-genesis issuance in the first place, so
there is nothing for a halving schedule to act on.

### Continuing/Inflationary Emission

Rejected in favor of a one-time genesis issuance. HNChain still needs a
validator security budget once any genesis-funded runway is spent, but
this ADR resolves that via existing-value transaction fees (§"Validator
Rewards And Transaction Fees," above) rather than newly minted HNCOIN,
per the whitepaper's own anticipated fallback (§12.8).

### Modeling Burn As A Transfer To A Dead Address

Rejected because a conventional address, even one nobody currently
holds a known key for, is not a protocol guarantee that funds sent
there are unspendable — it relies on nobody ever finding or brute-
forcing a matching key, not on a consensus rule. Burn must be an
explicit, protocol-enforced state transition that removes value from
`CURRENT_SUPPLY` itself.

### Governance-Controlled Discretionary Minting

Rejected because it reintroduces exactly the "hidden or discretionary
funding mechanism" the whitepaper explicitly says HNChain should avoid
(§12.6) and defeats the purpose of a fixed-supply guarantee — a
supply cap that governance can vote around is not a cap.

## Alternatives Considered

### Uncapped Supply With Governance-Set Emission

Advantages:

- flexible, can respond to changing validator economics over time

Disadvantages:

- no fixed monetary guarantee for holders
- emission rate becomes a recurring governance-capture target
- contradicts the "no discretionary funding mechanism" principle
  (whitepaper §12.6) this project already holds for treasury design

### Smaller Genesis Supply With Continuing Emission (e.g. Bitcoin/Ethereum-style)

Advantages:

- familiar model with extensive precedent
- built-in, automatic validator funding independent of fee volume

Disadvantages:

- requires an accepted emission curve, which does not exist and would
  itself need extensive economic modeling
- dilutes holders unless offset by burn or demand growth
- the whitepaper's own stated direction (§12.2) already leans toward
  fixed supply

## Security Considerations

Validator security budget after genesis-funded runway:

- Risk: a fixed supply with no emission, combined with insufficient fee
  revenue, leaves validators economically unable to secure the network
  long-term.
- Mitigation: transaction fees are the designated funding source
  (§"Validator Rewards And Transaction Fees"); the exact fee
  amount/market model remains an ADR-0023 Open Decision requiring
  economic modeling before acceptance.

Genesis allocation key compromise:

- Risk: compromise of the Founder, Reserve, or Community allocation's
  controlling key(s) allows an attacker to spend those funds.
- Mitigation: compromise of an allocation account's key never grants
  minting authority — the fixed-supply invariant holds regardless of
  which account controls which existing funds. Multisig/hardware-
  wallet/timelock/vesting mechanisms for these three accounts are
  explicitly deferred to a future genesis-security specification, not
  decided by this ADR.

Genesis misconfiguration:

- Risk: a genesis construction bug produces an allocation sum that
  does not equal `GENESIS_SUPPLY`, silently creating or destroying
  HNCOIN before the chain even starts.
- Mitigation: "Genesis Allocation Completeness" (Normative Rules,
  above) — genesis construction must reject any non-matching sum.

Perceived centralization from the Founder allocation:

- Risk: a 10% founder allocation, even if transparently documented,
  invites legitimacy scrutiny regardless of technical correctness.
- Mitigation: transparency (the allocation is fixed, public, and part
  of this accepted ADR rather than an implementation detail) and
  deferred vesting/lockup terms, to be specified separately rather than
  left silently unrestricted.

## Compatibility

Every item listed under "Protocol Upgrade Constraints" (Normative
Rules, above) is a monetary policy change and requires a new accepted
ADR superseding or explicitly amending this one — never an ordinary
implementation change, matching the bar ADR-0015 already sets for
activating slashing and ADR-0019 sets for a storage backend switch that
would change anything backend-independence guarantees. Values decided
here (`MAX_SUPPLY`, the three genesis allocation amounts,
`POST_GENESIS_MINT = FORBIDDEN`, `INFLATION = 0%`) become
consensus-critical once this ADR reaches Accepted status.

## Open Decisions

Two items this ADR originally left open are now resolved by ADR-0023's
own economic-parameter batch pass, not by this ADR: HNCOIN's atomic
unit/decimals (**resolved: `DECIMALS = 9`, atomic unit `hnit`** — ADR-
0023, "Decided: HNCOIN Decimals And Atomic Unit"), fee burn ratio /
validator fee distribution split (**resolved: 70% validator / 30%
burn** — ADR-0023, "Decided: Validator Reward / Fee Distribution"), and
protocol treasury funding (**resolved: none — the Liquidity &
Ecosystem Reserve already covers this role** — ADR-0023, "Decided: No
Protocol Treasury"). This ADR's own text is not amended to restate
those values, per ADR-0023's own "One Owning Document" rule.

Deliberately not decided by this ADR (owned elsewhere, or genuinely
separate follow-up work):

- the fixed fee rate itself and minimum fee floor (ADR-0023 — model
  decided there, "Decided: Fee Model For `tx_version = 1`"; the amount
  is not)
- burn's concrete `tx_type`/wire mechanism (this ADR decides the
  invariant burn must satisfy; ADR-0006's `tx_type` registry has no
  `burn` entry yet)
- genesis allocation account key material, multisignature
  configuration, hardware-wallet requirements, timelocks, and Founder
  allocation vesting schedule (genesis-security specification, not yet
  written)
- Community & Airdrop distribution schedule and mechanism
- Liquidity & Ecosystem Reserve spending authorization and use-case
  approval process
- supply accounting mechanism (explicit running counter vs.
  reconstructed by summing account balances — an ADR-0007
  implementation-specification choice, not a monetary-policy one)

## Related Specifications

- `docs/whitepaper/HNChain-Whitepaper-v0.1-draft.md` (Chapter XIII,
  "Economic Model" — this ADR supersedes its illustrative, explicitly
  non-accepted `100,000,000 HNC` figure)
- `docs/specs/core/genesis.md` (genesis allocation construction —
  currently Draft, defers "genesis allocation" to this ADR)
- `docs/adr/ADR-0023-tokenomics-and-economic-model.md`
