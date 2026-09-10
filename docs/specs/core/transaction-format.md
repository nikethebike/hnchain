# HNChain Core Specification: Transaction Format

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## 1. Scope

This document specifies the conceptual HNChain transaction format.

It defines transaction envelope fields, transaction identifiers, signing
payloads, validation stages, transaction type boundaries, access lists, and
security requirements.

This document does not finalize HNCS schemas, the fee market, HNVM payloads,
state transition rules, mempool policy, receipt format, or block inclusion
rules.

This specification is constrained by:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0001-account-state-model.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0003-address-format.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0006-transaction-format.md`

## 2. Design Goals

- One canonical transaction encoding.
- Explicit transaction versioning.
- Replay resistance across accounts, chains, networks, and protocol versions.
- Cheap rejection of invalid transactions before expensive execution.
- Support for multiple authorization models.
- Compatibility with deterministic parallel execution.
- Compatibility with future transaction types.

## 3. Transaction Envelope

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

All fields are consensus-relevant unless a future schema explicitly states
otherwise.

The canonical binary encoding is HNCS.

## 4. Field Requirements

### 4.1 Transaction Version

`tx_version` identifies the transaction envelope format. `u16`, a
Structure Version matching the convention used everywhere else in this
project. Decided in ADR-0006, "Versioned Envelope".

Unknown transaction versions are rejected unless protocol upgrade rules define
acceptance or migration behavior.

### 4.2 Chain ID And Network ID

`chain_id` identifies the HNChain chain. `uint8`, a small closed registry
grown only through explicit governance action (`0x00` reserved/invalid,
`0x01` the HNChain lineage) — decided in ADR-0006, "Chain And Network
Binding".

`network_id` identifies the network environment, such as mainnet, testnet, or
devnet. `uint16`, reusing `AddressPayload.network_id`'s registry unchanged
(ADR-0003, "Decision 4"): `0x0001` mainnet, `0x0002` testnet,
`0x0003`-`0x7FFF` reserved/future-registered, `0x8000`-`0xFFFF`
self-assigned devnet range.

Both fields are included in signing payloads and replay protection.

### 4.3 Transaction Type

`tx_type` identifies payload semantics. `u8`, closed registry (ADR-0006,
"Transaction Type"):

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

Every active transaction type requires a dedicated schema and validation
rules — this registry assigns identifiers only, it does not activate any
type's payload behavior.

### 4.4 Sender

`sender` is a canonical address payload. `bytes32` — the sender's own
`address_body` (ADR-0003), not a full `AddressPayload`. Decided in
ADR-0006, "Sender".

Display address strings are not used in consensus transaction encoding.

### 4.5 Nonce

Nonce prevents replay and defines account transaction ordering.

`u64`, initial `0` (`NonceValueV1`, account-state.md §4.4). A nonce is
consumed on inclusion (passing precheck), not on successful execution —
a failed execution still consumes its nonce, only its own state effects
revert. Inclusion requires the transaction's nonce to exactly equal the
sender's current on-chain nonce (strictly increasing, gap-free per
sender). Replay protection scope is `(chain_id, network_id,
sender_address, nonce)`. Nonce ordering serializes only same-sender
transactions; cross-sender conflicts are an access list concern, not a
nonce concern. Decided in ADR-0006, "Nonce".

Still open:

- interaction with multi-signature (depends on account permission rules,
  not yet activated — account-state.md §4.5)

### 4.6 Fee Limit

`fee_limit` constrains transaction resource consumption. `u128`, matching
the width convention `native_balance`/`amount` already use
(account-state.md §4.3/§4.7). Decided in ADR-0006, "Fees" — mechanism
only, not an amount.

`fee_limit` is a cap, not an exact charge (the field's own name, not a
metering formula): the sender pays, up to this cap, for resources
actually consumed; a failed execution still owes a fee for what it
consumed before failing, mirroring nonce's already-decided
still-consumed-on-failure rule. Third-party fee sponsorship is deferred,
not decided either way, pending account-state.md §4.5 Permission State.

The final fee structure may include base fees, execution fees, storage fees,
priority fees, refunds, and burn rules — all of this is amount/market/policy,
which is out of scope for this specification and gated on a future
economics and HNVM metering specification.

This specification does not finalize the fee market.

### 4.7 Validity Window

`validity_window` limits when a transaction may be included. Decided in
ADR-0006, "Validity Window": height-based, not epoch-based — epochs are
too coarse (validator-set/protocol-parameter periods) to usefully bound
how long an ordinary transaction may sit unconfirmed.

```text
ValidityWindowV1
  optional u64 min_height
  optional u64 max_height
```

Both bounds reference `hn_core::BlockHeight` and are independently
optional (absent `min_height` = valid from genesis; absent `max_height`
= no expiry) — not a sentinel value, and not nested in an outer
`optional`, since "no window" is already both bounds absent.

Consensus-time or block-time fields require a separate time semantics
specification.

### 4.8 Access List

`access_list` describes expected state access.

```text
AccessListV1
  set<bytes32> reads
  set<bytes32> writes
```

Access entries may reference:

- accounts
- contract storage keys
- asset identifiers
- validator state
- protocol module state

**Decided: hint-only, not consensus-enforced** (ADR-0006, "Access List").
A mismatch between declared and actual state access is never a validity
error; execution engines may use `access_list` for scheduling, conflict
detection, and fee estimation, but must always verify actual access
independently. Follows Ethereum's EIP-2930 model rather than Solana's
protocol-enforced one, since HNChain's `contract_call` implies dynamic,
EVM-like contract execution through a future HNVM whose access patterns
cannot always be statically predicted — deciding strict enforcement now
would commit that undesigned execution model to a Solana-like
upfront-declaration discipline. A future HNVM-specific ADR may still add
stricter enforcement for specific `tx_type`s that support it.

**Decided: entry structure** (ADR-0006, "Access List"). Each entry is a
`state_key` (ADR-0007) — the same 32-byte value the state tree already
uses as its one uniform leaf address for every domain, including all
five reference categories listed above (`accounts`, `contract_storage`,
`assets`, `validators`, and every protocol module, ADR-0007 "State
Domains"). This is not an independent design choice: it falls directly
out of ADR-0007 already being Accepted and already unifying all of
these under one tree, so access list entries need no domain-specific
structure or a second registry — an execution engine comparing two
transactions' access sets is a flat 32-byte comparison regardless of
domain. `reads` and `writes` are independent bounded canonical HNCS
sets, each capped at `MAX_ACCESS_LIST_ENTRIES = 256` (an implementation
DoS bound, not derived — meaningful only because of the hint-only model
above, since a declared entry can never make a valid transaction
invalid). A key may appear in both sets; a read-modify-write is not a
conflict with itself.

## 5. Payloads

Payload schema is selected by `tx_type`.

Every payload must define:

- payload version
- HNCS schema
- validation preconditions
- required permissions
- state transition behavior
- event behavior
- receipt behavior
- failure behavior

Payload bytes must be bounded.

## 6. Signatures

Transactions contain one or more signature envelopes as defined by cryptographic
identity specifications.

Signature verification must check:

- algorithm lifecycle
- key role
- key binding to sender account
- signature canonicality
- signing payload
- verification context

## 7. Signing Payload

The signing payload is a canonical representation of transaction intent.

Conceptual structure:

```text
TransactionSigningPayload
  chain_id
  network_id
  tx_version
  tx_type
  sender
  nonce
  fee_limit
  validity_window
  access_list
  payload
```

`protocol_name` is not a field: domain separation is already provided by
the hash construction's own `domain_tag` (see below), so a redundant
in-payload string would duplicate that guarantee rather than add one
(ADR-0006, "Signing Payload").

Signatures are not included inside the signing payload unless a specific
multi-signature scheme defines nested signing behavior.

**Decided: hash mechanism** (ADR-0006, "Signing Payload"):

```text
signing_digest = HASH_PROFILE_0x0001(
  "hnchain.transaction.signing.v1", HNCS(TransactionSigningPayload))
```

Every field's shape except `payload` is now decided (§4.1-§4.8);
`payload`'s per-`tx_type` schema (§5) is the one remaining open piece.

## 8. Transaction ID

The transaction ID is a domain-separated hash of canonical transaction bytes.

**Decided** (ADR-0006, "Transaction ID"):

```text
tx_id = HASH_PROFILE_0x0001(
  "hnchain.transaction.id.v1", HNCS(TransactionEnvelope))
```

Reuses `HASH_PROFILE_0x0001` (ADR-0005) with its own reserved domain tag
— no separate transaction ID hash profile. Unlike the signing payload,
`tx_id` commits to the full envelope including `signatures`, which is
what makes transaction malleability a meaningful risk to mitigate at the
`tx_id` level (§11).

## 9. Validation Pipeline

Transaction validation proceeds from cheap checks to expensive checks:

```text
bytes
  -> size limits
  -> HNCS decode
  -> version check
  -> chain and network check
  -> transaction type check
  -> signature verification
  -> nonce precheck
  -> fee precheck
  -> access list precheck
  -> state transition execution
```

`size limits` runs first, on raw encoded bytes, before HNCS decode —
`MAX_TRANSACTION_SIZE` (ADR-0006, "Transaction Size Limit") must reject
an oversized blob before spending CPU decoding it, not after.

Consensus validity is defined by block validation and state transition rules.

Mempool admission policy may reject transactions that would still be invalid or
unwanted locally, but mempool policy must not redefine block validity.

## 10. Module Boundaries

```text
Wallet / SDK / RPC
      |
      v
Transaction Construction
      |
      v
Canonical Transaction Bytes
      |
      v
Validation Layer -----> Cryptographic Identity
      |
      v
State Transition Engine -----> HNVM
      |
      v
State Database
```

Boundary rules:

- Wallets construct transactions but do not define validity.
- RPC transports transactions but does not reinterpret canonical bytes.
- Mempool manages pending transactions but does not define consensus rules.
- Validation owns transaction validity checks.
- State transition engine owns consensus state mutation.
- HNVM executes only through defined state access interfaces.

## 11. Security Requirements

- Transactions must have canonical encoding.
- Transactions must bind to chain and network identifiers.
- Signatures must bind to verification context.
- Transaction IDs must use domain-separated hash profiles.
- Unknown versions and transaction types must be rejected before activation.
- Transaction sizes must be bounded: `MAX_TRANSACTION_SIZE = 262144` bytes
  (256 KiB), decided in ADR-0006, "Transaction Size Limit" — checked on
  raw encoded bytes before HNCS decode.
- Payload sizes must be bounded (per-type; still open, §5).
- Access lists must not cause nondeterministic execution.
- Fee prechecks must limit resource exhaustion.
- Failed execution behavior must be deterministic.

## 12. Open Architecture Decisions

- final HNCS schema
- final fee model (mechanism decided; see §4.6 — amount, refunds,
  distribution, burn policy, and priority market remain economic
  decisions)
- final `payload` schemas per `tx_type` (§5) — the one remaining
  envelope/signing-payload field; every other field is now decided
- final receipt schema
- final event schema
- final mempool policy boundaries
