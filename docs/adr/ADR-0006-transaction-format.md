# ADR-0006: Transaction Format

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0003: Address Format
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms

Supersedes: None

## Context

Transactions are the primary user intent objects that cause state transitions in
HNChain.

The transaction format must bind together account state, cryptographic
identity, addresses, canonical serialization, hash profiles, fees, nonce rules,
authorization, and future HNVM execution.

Transactions must be deterministic, replay-resistant, versioned, and safe to
validate before expensive execution.

## Decision

HNChain uses versioned transaction envelopes.

Conceptual structure:

```text
TransactionEnvelope
  tx_version
  chain_id
  network_id
  tx_type
  sender
  nonce
  fee_limit
  validity_window
  access_list
  payload
  signatures
```

The transaction identifier is computed from the canonical HNCS encoding of the
transaction under a transaction ID hash profile.

The signing payload is computed from a canonical subset of the transaction under
a transaction signing hash profile and mandatory verification context.

The final canonical field encoding is defined in the transaction specification
and HNCS schemas.

## Normative Rules

### Versioned Envelope

Every transaction includes `tx_version`.

Nodes must not infer transaction format from byte length, payload shape, RPC
method, wallet version, or signature algorithm.

**Decided: `tx_version` type.** `u16`, a Structure Version in ADR-0022's
sense, matching the width convention already used for every other
structure version field in this project (`envelope_version`,
`nonce_version`, and similar).

### Chain And Network Binding

Every transaction binds to `chain_id` and `network_id`.

Transactions valid on one HNChain network must not be replayable on another
network.

**Decided: `chain_id` format.** `chain_id` is `uint8`, a small closed
registry grown only through explicit governance action, not the
closed-registry-plus-self-assigned-range model `network_id` uses (ADR-0003,
"Network Separation").

```text
0x00  reserved, invalid
0x01  the HNChain lineage (initial genesis)
```

The two fields serve different purposes and change at different rates:
`network_id` identifies the environment (mainnet/testnet/devnet) and needs
room for many concurrent, disposable, self-assigned devnet instances (see
ADR-0003's reasoning for `network_id` being `uint16`). `chain_id` identifies
the HNChain protocol lineage itself and changes only on an irreconcilable
governance fork — a rare, deliberate, centrally-coordinated event, not
something anyone spins up unilaterally. There is no legitimate scenario
where two independently-created `chain_id` values need to coexist without
a governance decision behind each one, so there is no need for a
self-assigned range the way devnets needed one for `network_id`. `uint8`
gives 255 usable values, which is far more headroom than a rare, deliberate
event will ever need.

`chain_id` is not part of `AddressPayload` (ADR-0003, "no `chain_id` in
`AddressPayload`"); it appears here in `TransactionEnvelope` and in
`BlockHeader` (ADR-0008) for replay protection across a lineage split.
This is the canonical decision for `chain_id`'s format — ADR-0008
references it rather than redeciding it, since ADR-0000's Required ADR
Dependency Order places ADR-0006 before ADR-0008 and a block-format
document cannot be a dependency of a transaction-format document.

**Decided: `network_id` format.** `network_id` is `uint16`
(ADR-0003, "Decision 4"): `0x0001` mainnet, `0x0002` testnet,
`0x0003`-`0x7FFF` reserved/future-registered, `0x8000`-`0xFFFF`
self-assigned devnet range. `TransactionEnvelope.network_id` reuses
`AddressPayload.network_id`'s registry unchanged — this is the same value,
not a parallel one, so a mismatch between a sender's address and the
transaction's own `network_id` is a validation error, not two independent
facts to reconcile.

### Transaction Type

Every transaction includes `tx_type`.

Initial conceptual types:

- `transfer`
- `contract_deploy`
- `contract_call`
- `stake`
- `unstake`
- `validator_update`
- `governance`
- `permission_update`
- `system`

Unknown transaction types are rejected unless activated by protocol upgrade
rules.

**Decided: `tx_type` registry.** `u8`, closed for `tx_version = 1`
(matching the "rejected unless activated by protocol upgrade" rule above;
not a self-assigned range — there is no legitimate scenario for two
independently-created `tx_type` values to coexist, the same reasoning
already used for `chain_id`).

```text
0x00  reserved, invalid
0x01  transfer
0x02  contract_deploy
0x03  contract_call
0x04  stake
0x05  unstake
0x06  validator_update
0x07  governance
0x08  permission_update
0x09  system
```

This registry assigns identifiers only; it does not decide any payload's
schema, validation rules, or activation status (Payload, below, remains
open per-type). `permission_update` and the `governance`/`stake`/
`unstake`/`validator_update`/`system` types in particular reference
account/protocol capabilities (Permission section, protocol module
domains) that are not yet activated — their `tx_type` value is reserved
here, not their behavior.

### Sender

The sender is a canonical address payload, not a display string.

The sender account must authorize the transaction through signatures or another
approved authorization proof.

**Decided: `sender` field shape.** `bytes32`, the sender's own
`address_body` (ADR-0003) — not a full `AddressPayload`. This matches
`EnvelopeValueV1.address`'s precedent (account-state.md §4.1) and avoids
redundancy: `AddressPayload.network_id` would duplicate the envelope's
own already-bound `network_id` field, and `address_namespace` would
always be `account` (`0x01`) since only accounts submit transactions —
contracts and validators are invoked by transactions, they do not send
them. Everything needed to re-derive and verify `sender` against a
signature is already present elsewhere in the envelope
(`network_id` here, `algorithm_id` in the matching `SignatureEnvelope`,
ADR-0002) or is currently a single fixed value (`address_version = 1`).

### Nonce

Transactions include nonce state for replay protection and account ordering.

The nonce model must define:

- nonce width
- initial nonce
- increment rules
- behavior for failed execution
- ordering rules
- replay protection domain
- interaction with parallel execution

**Decided: nonce model**, except fee behavior (owned by Fees, below).
Storage width and initial value (`u64`, initial `0`) are already decided
in `NonceValueV1` (account-state.md §4.4); this closes the remaining
transaction-validation semantics that section left to this ADR.

- **Increment rule.** A transaction's nonce is consumed exactly once it
  passes precheck (signature, nonce match, and fee-affordability checks)
  and is included in a block — regardless of whether the transaction's
  own execution later succeeds or reverts. A transaction that fails
  precheck is never included and never consumes a nonce.
- **Behavior for failed execution.** If execution fails after inclusion,
  only the transaction's own state effects are reverted; the nonce
  increment from the rule above is not undone. This is required so a
  failed execution cannot be resubmitted or replayed under the same
  nonce. A fee is owed in this case too (Fees, below) — the amount is not
  decided by this rule, only that the obligation survives.
- **Ordering rules.** A sender's transactions must be included in
  strictly increasing, gap-free nonce order: a transaction is valid for
  inclusion only when its nonce exactly equals the sender's current
  on-chain nonce at execution time. A mempool may hold and reorder
  transactions with higher nonces locally, but block validity does not.
- **Replay protection domain.** The uniqueness scope for a signed
  transaction is `(chain_id, network_id, sender_address, nonce)` — chain
  and network binding (decided above) plus a nonce that can only ever be
  consumed once per sender together make a valid signed transaction
  replayable nowhere else and at most once.
- **Interaction with parallel execution.** Nonce ordering serializes only
  transactions from the *same* sender (each sender has its own
  independent nonce sequence). Transactions from different senders are
  not ordered relative to each other by nonce at all; any shared-state
  conflicts between them are resolved by the access list mechanism
  (Access List, below), not by nonce.

This account-based sequential-nonce model follows directly from ADR-0001
(Extended Account-Based State Model) already being the accepted account
model — it is not an independent design choice among live alternatives
for this project.

### Fees

Transactions include fee limits or equivalent resource-payment constraints.

The fee model must define:

- fee payer
- maximum fee
- resource metering units
- failed transaction fees
- refunds
- validator distribution
- burn policy
- storage costs
- priority behavior, if any

The transaction format must not assume a final fee market before the economics
and HNVM metering specifications are accepted.

**Decided: fee mechanism only** — never an amount, split, or market
model; those remain owned by a future tokenomics specification and HNVM
metering specification, per this section's own constraint above and the
project's standing rule that no economic parameter (fee amount, burn
percentage, distribution split — even an explicit zero) may be encoded
before such a specification is accepted.

- **`fee_limit` type.** `u128`, matching `native_balance`/`amount`'s width
  convention (account-state.md §4.3/§4.7) for the same overflow-headroom
  reason — not an economic decision.
- **Fee payer.** The sender pays. Third-party fee sponsorship (a payer
  distinct from `sender`) is deferred, not decided either way: it would
  need its own authorization rule, and account-state.md §4.5 Permission
  State — the natural place such a rule would live — is itself not yet
  activated (account-state.md, deferred this session).
- **`fee_limit` is a cap, not an exact charge.** The field is named
  `fee_limit`, not `fee` or `fee_amount`: a transaction declares the
  maximum it is willing to pay, and the amount actually owed is
  determined by resource consumption up to that cap — this follows from
  the field's own name, not from deciding any metering formula. Whether
  and how the unused difference is refunded depends on resource metering
  units (still open, HNVM-gated); this decision only fixes that a
  cap/consumption relationship exists, not its arithmetic.
- **Failed execution still incurs a fee obligation.** A transaction that
  fails during execution (as opposed to failing precheck) still owes a
  fee for the resources it consumed before failing — mirroring the
  already-decided nonce rule (Nonce, above) that a failed execution still
  consumes its nonce. This closes the "whether a fee is charged" question
  the Nonce decision explicitly deferred to here. The *amount* owed on
  failure is not decided — that depends on metering units, same as the
  success case.

Resource metering units, refund arithmetic, validator distribution, burn
policy, storage costs, and priority-fee market behavior remain fully
open, gated on the economics and HNVM metering specifications this
section already requires.

### Validity Window

Transactions may include a validity window to limit how long they can be
included.

The validity window must use consensus-defined height, epoch, or time semantics.

It must not depend on local node wall-clock time.

**Decided: height-based, not epoch-based.**

```text
ValidityWindowV1
  optional u64 min_height
  optional u64 max_height
```

`min_height`/`max_height` reference `hn_core::BlockHeight` (already `u64`).
Epoch-based bounds (`hn_core::Epoch`, also already `u64`) are not used for
this general mechanism: epochs span many blocks (validator-set/
protocol-parameter periods), far too coarse a granularity for bounding how
long an ordinary transaction may sit unconfirmed — the validity window's
purpose here, distinct from nonce's replay protection. A future
epoch-scoped transaction type (for example `validator_update`) may still
reference `epoch` in its own payload; that is a per-payload decision (§5),
not this general envelope field.

Both bounds are independently optional (HNCS `optional`, ADR-0004), not a
sentinel value: `min_height` absent means no lower bound (valid from
genesis); `max_height` absent means no expiry. Nesting `validity_window`
itself in another `optional` would be redundant — "no window" is already
expressible as both bounds absent.

### Access List

Transactions may declare read and write access sets.

Access lists support deterministic validation, fee estimation, conflict
detection, and parallel execution.

If access lists are consensus-enforced, the protocol must define rejection or
fallback behavior for undeclared access.

If access lists are hints only, they must not affect consensus validity.

**Decided: hint-only, not consensus-enforced.** A mismatch between a
transaction's declared `access_list` and its actual state access during
execution is never a validity error. Execution engines may use
`access_list` for scheduling, conflict detection, and fee estimation, but
must always verify actual access independently and fall back to
sequential or re-execution semantics on an undeclared-access conflict,
never reject the transaction for the mismatch itself.

This follows Ethereum's EIP-2930 model, not Solana's strict
protocol-enforced one. The two differ in what they demand of the
execution environment, not just in strictness: Solana's strict
enforcement works because its runtime *requires* programs to declare
every accessed account upfront — a programming model decision baked in
from day one. HNChain's `tx_type` registry already includes
`contract_call`, implying dynamic, EVM-like contract execution through a
future HNVM whose design does not exist yet; a general-purpose contract's
storage access can depend on runtime branching in ways that are not
always statically predictable, which is precisely why Ethereum treats its
own access lists as hints rather than a consensus boundary. Deciding
strict enforcement now would commit HNVM's future execution model to a
Solana-like account-declaration discipline before HNVM itself is
designed — a much larger, earlier architectural commitment than this
transaction-format decision should make. A future HNVM-specific ADR may
still add stricter, enforced access declarations for specific `tx_type`s
whose execution model supports it; this decision does not foreclose that,
it just does not assume it now.

**Decided: access list entry structure.**

```text
AccessListV1
  set<bytes32> reads
  set<bytes32> writes
```

Each entry is a `state_key` (ADR-0007) — the same 32-byte value the
state tree itself already uses as its one uniform leaf address, not a
bespoke "logical reference" type. This falls directly out of ADR-0007
already being Accepted, not an independent design choice: `accounts`,
`contract_storage`, `assets`, `validators`, and every governance/system/
bridge-mapped protocol module (the exact five reference categories
transaction-format.md §4.8 lists) are already leaves of the *same*
single global tree (ADR-0007, "State Domains"), each addressed by
`domain_id`/`section_id`-or-`extension_id`/`object_id`/`subkey` through
`state_key_core`/`state_key_extension` — both already produce this exact
32-byte `Digest`. Reusing it means an access list entry needs no
domain-specific structure or a second registry to cover "which kind of
thing is this" — the state tree already answers that, and an execution
engine comparing two transactions' access sets for conflicts is a flat
32-byte comparison regardless of what domain either entry came from.

`reads` and `writes` are independent bounded canonical sets (HNCS `set`,
ADR-0004 — sorted by encoded bytes, duplicates rejected), each capped at
`MAX_ACCESS_LIST_ENTRIES = 256`. A key may legitimately appear in both
(a read-modify-write is not a conflict with itself). The cap is an
implementation DoS bound, same class as `MAX_TRANSACTION_SIZE` and
`MAX_ASSET_HOLDINGS` — picked for headroom, not derived, and only
meaningful given the hint-only model decided above: since a declared
entry can never make a valid transaction invalid, the cap only needs to
bound processing cost, not correctness.

### Payload

The payload is typed by `tx_type`.

Every payload type must define:

- schema version
- authorization requirements
- validation preconditions
- state transition behavior
- fee behavior
- event and receipt behavior
- failure semantics

**Decided: `transfer` (`tx_type = 0x01`) payload.** The only `tx_type`
currently unblocked: it needs only `BalanceValueV1`/`AssetValueV1`
(account-state.md §4.3/§4.7, both decided) and this ADR's own already-
decided fields. Every other `tx_type` is parked below, each blocked on a
subsystem that does not exist yet.

```text
TransferPayloadV1
  u16          payload_version = 1
  bytes32      recipient
  optional u16 asset_id
  u128         amount
```

Fields:

- `payload_version`: `u16`, Structure Version, matching this project's
  convention.
- `recipient`: `bytes32`, an `address_body` — same shape as `sender`
  (Sender, above), for the same reasons.
- `asset_id`: `optional u16`. Absent means native HNCOIN (Balance
  State, account-state.md §4.3); present references a curated
  protocol-level/bridged asset (Asset State, §4.7, `assets` domain
  registry). Contract-defined assets are out of scope for `transfer`
  entirely — their balances live in `contract_storage` under the
  issuing contract, not the `accounts` domain's Asset section
  (account-state.md §4.7), so moving them is a `contract_call`, not a
  `transfer`.
- `amount`: `u128`, matching the balance/asset width convention. May be
  zero (a valid no-op transfer, not a validation error) — this follows
  Ethereum's permissive stance rather than inventing a new restriction
  with no derivable justification.

Authorization: the sender's own signing key only (ADR-0002's default —
"exactly one active signing key unless the owning object specification
defines a threshold or multisignature rule"); `transfer` defines no
such rule, and Permission State's own capabilities are not activated
(account-state.md §4.5), so there is nothing beyond the base signature
to require yet.

Validation preconditions: the referenced `asset_id`, if present, must
exist in the `assets` domain registry; the sender's applicable balance
(`native_balance` or `holdings[asset_id]`) must be at least `amount`.

State transition: debit `amount` from the sender's applicable section,
credit `amount` to the recipient's applicable section.

**Decided: implicit account creation.** If `recipient` has no existing
account state, the transfer creates it (Ethereum's model, not Solana's
explicit-creation-required one) — asked the user first, since this was
a genuine fork with real precedent on both sides. `EnvelopeValueV1`
(`account_type = Standard`, current `section_versions`), `NonceValueV1`
(`AccountNonce::INITIAL`), `BalanceValueV1`/`AssetValueV1` (zero/empty,
then credited), and `LifecycleValueV1` (`Created`) are all fully
specifiable today from already-decided schemas. **Open sub-item, not
resolved here**: the newly-created account's Permission and Metadata
section values are not decided, because those sections' own value
schemas are still deferred (account-state.md §4.5/§4.6, no concrete
driver yet) — implicit creation via `transfer` cannot be fully closed
until they are, independent of everything else decided above.

Failure semantics follow the already-decided Nonce/Fees rules: a
`transfer` that fails a validation precondition (for example
insufficient balance) still consumed its nonce and still owes a fee
(amount not decided, Fees above); only its own state effects revert.

Event and receipt behavior for `transfer` is not decided: the receipt
and event models themselves are still fully open (Open Decisions,
below) for every `tx_type`, not specific to `transfer`.

**Parked, each blocked on a subsystem that does not exist yet, not
merely unaddressed:**

- `contract_deploy`, `contract_call` (`0x02`, `0x03`): blocked on HNVM,
  which has no design yet.
- `stake`, `unstake`, `validator_update` (`0x04`-`0x06`): blocked on
  consensus and validator specifications, which do not exist yet, and
  likely on economic parameters (staking amounts, reward/slashing
  rules) owned by a future tokenomics spec.
- `governance` (`0x07`): blocked on a governance model, which does not
  exist yet.
- `permission_update` (`0x08`): blocked on account-state.md §4.5
  Permission State, itself explicitly deferred this session.
- `system` (`0x09`): scope not yet concrete — no protocol module
  operation has been specified that would use it.

### Signatures

Transactions include one or more signature envelopes.

Every signature must bind to:

- protocol name
- chain ID
- network ID
- transaction type
- transaction version
- sender or key reference
- signing purpose
- canonical signing payload

`signatures` is a list of `SignatureEnvelope` (ADR-0002, Accepted:
`envelope_version`, `algorithm_id`, `key_reference`, `signature`,
`verification_context`) — that container shape is already decided by
ADR-0002 and is not redecided here; this ADR decides what goes into
`verification_context` (Signing Payload, below), not the envelope that
carries it.

Multi-signature and threshold authorization require explicit account permission
rules before activation — deferred along with account-state.md §4.5
Permission State, which is itself not yet activated.

### Signing Payload

The signing payload is a canonical subset of the transaction: `sender`'s
signature verifies over this, not over the full `TransactionEnvelope`
(`signatures` cannot be included in its own signing payload).

**Decided: signing payload hash mechanism.**

```text
signing_digest = HASH_PROFILE_0x0001(
  "hnchain.transaction.signing.v1", HNCS(TransactionSigningPayload))
```

Reuses `HASH_PROFILE_0x0001` (ADR-0005) with the domain tag already
reserved for this purpose in ADR-0005's conceptual registry — the same
pattern ADR-0007 uses for every state-tree hash, not a second hash
profile.

`protocol_name` (listed in the "every signature must bind to" list above,
and in `TransactionSigningPayload`'s conceptual structure,
transaction-format.md §7) is **not** a field of `TransactionSigningPayload`
— it would duplicate what `DomainSeparatedHashInputV1`'s own
`domain_tag` (ADR-0005) already provides. `hnchain.transaction.signing.v1`
already says "this digest means HNChain transaction signing intent, and
nothing else can produce or accept it" for free; a redundant string field
inside the payload would repeat that guarantee without adding one (same
class of redundancy as `checksum_profile` inside `AddressPayload`,
ADR-0003 Decision 5). Likewise "signing purpose" in that list is the
domain tag itself, not a separate field. "chain ID" and "network ID" are
the already-decided `chain_id`/`network_id` fields (above), not
duplicated identifiers.

`TransactionSigningPayload` mirrors `TransactionEnvelope` minus
`signatures`. Every field's shape except `payload` is now decided (above
and in Access List, Fees, Validity Window); `payload`'s own per-`tx_type`
schema (§5) is the one remaining open piece of either structure.

### Transaction ID

Transaction ID is derived from canonical bytes using a transaction ID hash
profile.

Transaction ID must not be computed over JSON, display strings, RPC request
objects, or local memory layouts.

**Decided: transaction ID hash mechanism.**

```text
tx_id = HASH_PROFILE_0x0001(
  "hnchain.transaction.id.v1", HNCS(TransactionEnvelope))
```

Same reasoning and mechanism as the signing payload above: reuses
`HASH_PROFILE_0x0001` with its own already-reserved domain tag, no
second hash profile. Unlike the signing payload, `tx_id` commits to the
*full* `TransactionEnvelope`, including `signatures` — this is what
makes transaction malleability meaningful to guard against at all (see
Security Considerations, "Transaction malleability"): if `tx_id` excluded
`signatures`, a different valid signature over the same intent would not
change the ID, and the malleability risk framing would not apply to
`tx_id` in the first place. `TransactionEnvelope`'s own field list has
the same one remaining gap as the signing payload above: `payload`'s
per-`tx_type` schema.

### Transaction Size Limit

**Decided: `MAX_TRANSACTION_SIZE = 262144` bytes (256 KiB)**, checked on
raw encoded bytes as the first validation stage, before HNCS decode.

An implementation-level resource/DoS bound, not a derived consensus
value — the same class of decision as `hn-crypto`'s
`PUBLIC_KEY_MAX_LEN` and `hn-state`'s `MAX_ASSET_HOLDINGS`: picked for
generous headroom over any currently-imaginable payload (the largest
candidate, `contract_deploy` bytecode, comfortably fits well under this
bound for realistic contracts) rather than derived bottom-up, since
`payload`'s own per-type size limits are not decided yet (§5, still
open). Raise this bound later if a real payload type needs more; never
lower it silently once transactions exist on any live network.

### Validation Before Execution

Nodes must perform cheap validation before expensive execution.

Initial validation stages:

```text
bytes
  -> size limits
  -> HNCS decode
  -> version check
  -> chain and network check
  -> transaction type check
  -> signature verification
  -> nonce and fee precheck
  -> access list precheck
  -> state transition execution
```

Invalid transactions must be rejected before HNVM execution where possible.

## Rejected Options

### JSON Transactions As Consensus Objects

Rejected because JSON creates unacceptable ambiguity in field ordering, number
representation, Unicode behavior, and canonical hashing.

JSON may be used for RPC requests that encode or submit canonical transaction
bytes.

### Transaction Format Without Version

Rejected because HNChain must support long-term protocol evolution.

### Signature Algorithm Inferred From Public Key Length

Rejected because it breaks algorithm agility and creates downgrade and parsing
risks.

### Global Sequential Execution Only

Rejected as a long-term design assumption because HNChain targets parallel
execution where deterministic access sets allow it.

Sequential execution may still be used as a conservative implementation mode or
fallback.

## Security Considerations

Replay attacks:

- Risk: transaction is valid on multiple chains or after account state changes.
- Mitigation: chain/network binding, nonce domain, transaction type, and signing
  context.

Signature confusion:

- Risk: the same signature authorizes another object or transaction type.
- Mitigation: mandatory verification context and canonical signing payload.

Fee exhaustion:

- Risk: attackers submit transactions that force expensive validation or
  execution without paying.
- Mitigation: fee limits, prechecks, size limits, and deterministic metering.

Parallel execution conflicts:

- Risk: conflicting transactions execute nondeterministically.
- Mitigation: access lists, conflict detection, deterministic scheduling, or
  conservative fallback.

Mempool divergence:

- Risk: nodes maintain different mempool policies and users see inconsistent
  preconfirmation behavior.
- Mitigation: mempool policy is local, while block validity is consensus-defined.

Transaction malleability:

- Risk: a transaction can be modified without changing semantic intent.
- Mitigation: transaction IDs and signatures commit to canonical HNCS bytes and
  explicitly defined signing payloads.

## Compatibility

Adding a new transaction type can be backward-compatible only if:

- it has a unique identifier
- canonical schema is defined
- validation rules are specified
- unsupported nodes reject it deterministically until activation
- activation rules are defined

Changing the meaning of an existing transaction version is a major protocol
change.

## Open Decisions

- final transaction envelope fields (every field except `payload` is now
  decided above: `chain_id`/`network_id`/`tx_version`/`tx_type`/`sender`/
  `validity_window`/`fee_limit`'s type/`access_list`; `payload` remains
  open for 8 of 9 `tx_type`s — `transfer`'s payload is now decided,
  §5 — each parked on a named blocker, not merely unaddressed)
- newly-created accounts' Permission/Metadata initial values (`transfer`
  implicit creation, §5) — blocked on account-state.md §4.5/§4.6
- final fee model (mechanism decided above — type, payer, cap-not-exact,
  failed-execution obligation; amount, refund arithmetic, validator
  distribution, burn policy, storage costs, and priority-fee market
  behavior remain economic decisions owned by a future tokenomics and
  HNVM metering specification)
- receipt model
- event model
- signing payload schema (hash mechanism and full field list now decided
  above; only `payload`'s own per-`tx_type` shape remains open, same
  caveat as the envelope)
- multi-signature activation model
- threshold authorization model
- mempool admission policy

## Related Specifications

- `docs/specs/core/transaction-format.md`
- `docs/specs/core/state-tree.md`
