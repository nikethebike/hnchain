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
  nonce. Whether a fee is charged in this case is decided by the Fee
  model (still open, see Fees below), not by this rule.
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

`TransactionSigningPayload`'s full field list is not finalized here: it
mirrors `TransactionEnvelope` minus `signatures`, so it cannot be closed
before `fee_limit` and `access_list` are (Fees, Access List — still open;
`validity_window`'s shape is now decided above).

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
`tx_id` in the first place. `TransactionEnvelope`'s own full field list
is likewise not finalized until `fee_limit`/`access_list` are, same
caveat as above.

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

- final transaction envelope fields (`chain_id`/`network_id`/`tx_version`/
  `tx_type`/`sender`/`validity_window` now decided above; `fee_limit`
  still fully open, `access_list`'s enforcement model is decided but its
  concrete structure is not, `payload` still open)
- final fee model
- access list structure (enforcement model is decided above — hint-only;
  the concrete `reads`/`writes` entry shape referencing accounts,
  contract storage keys, asset identifiers, validator state, and
  protocol module state is not)
- receipt model
- event model
- signing payload schema (hash mechanism now decided above; full field
  list still blocked on fee_limit and access list structure)
- multi-signature activation model
- threshold authorization model
- mempool admission policy

## Related Specifications

- `docs/specs/core/transaction-format.md`
- `docs/specs/core/state-tree.md`
