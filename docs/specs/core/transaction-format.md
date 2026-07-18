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

`tx_version` identifies the transaction envelope format.

Unknown transaction versions are rejected unless protocol upgrade rules define
acceptance or migration behavior.

### 4.2 Chain ID And Network ID

`chain_id` identifies the HNChain chain.

`network_id` identifies the network environment, such as mainnet, testnet, or
devnet.

Both fields are included in signing payloads and replay protection.

### 4.3 Transaction Type

`tx_type` identifies payload semantics.

Initial conceptual registry:

```text
transfer
contract_deploy
contract_call
stake
unstake
validator_update
governance
permission_update
system
```

Every active transaction type requires a dedicated schema and validation rules.

### 4.4 Sender

`sender` is a canonical address payload.

Display address strings are not used in consensus transaction encoding.

### 4.5 Nonce

Nonce prevents replay and defines account transaction ordering.

Open requirements:

- fixed-width integer type
- initial value
- increment point
- behavior on validation failure
- behavior on execution failure
- interaction with multi-signature
- interaction with parallel execution

### 4.6 Fee Limit

`fee_limit` constrains transaction resource consumption.

The final fee structure may include base fees, execution fees, storage fees,
priority fees, refunds, and burn rules.

This specification does not finalize the fee market.

### 4.7 Validity Window

`validity_window` limits when a transaction may be included.

Candidate forms:

```text
min_height
max_height
min_epoch
max_epoch
```

Consensus-time or block-time fields require a separate time semantics
specification.

### 4.8 Access List

`access_list` describes expected state access.

Conceptual structure:

```text
AccessList
  reads
  writes
```

Access entries may reference:

- accounts
- contract storage keys
- asset identifiers
- validator state
- protocol module state

The access list enforcement model is open.

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
  protocol_name
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

Signatures are not included inside the signing payload unless a specific
multi-signature scheme defines nested signing behavior.

## 8. Transaction ID

The transaction ID is a domain-separated hash of canonical transaction bytes.

Conceptual computation:

```text
tx_id = HASH_PROFILE_TRANSACTION_ID(HNCS(TransactionEnvelope))
```

The exact profile is defined by the hash algorithms specification.

## 9. Validation Pipeline

Transaction validation proceeds from cheap checks to expensive checks:

```text
bytes
  -> HNCS decode
  -> size limits
  -> version check
  -> chain and network check
  -> transaction type check
  -> signature verification
  -> nonce precheck
  -> fee precheck
  -> access list precheck
  -> state transition execution
```

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
- Transaction sizes must be bounded.
- Payload sizes must be bounded.
- Access lists must not cause nondeterministic execution.
- Fee prechecks must limit resource exhaustion.
- Failed execution behavior must be deterministic.

## 12. Open Architecture Decisions

- final HNCS schema
- final transaction type identifiers
- final nonce rules
- final fee model
- final validity window fields
- final access list enforcement
- final transaction size limits
- final receipt schema
- final event schema
- final transaction ID hash profile
- final signing payload hash profile
- final mempool policy boundaries
