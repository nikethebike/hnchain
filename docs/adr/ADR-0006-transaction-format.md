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

### Chain And Network Binding

Every transaction binds to `chain_id` and `network_id`.

Transactions valid on one HNChain network must not be replayable on another
network.

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

### Sender

The sender is a canonical address payload, not a display string.

The sender account must authorize the transaction through signatures or another
approved authorization proof.

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

### Access List

Transactions may declare read and write access sets.

Access lists support deterministic validation, fee estimation, conflict
detection, and parallel execution.

If access lists are consensus-enforced, the protocol must define rejection or
fallback behavior for undeclared access.

If access lists are hints only, they must not affect consensus validity.

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

Multi-signature and threshold authorization require explicit account permission
rules before activation.

### Transaction ID

Transaction ID is derived from canonical bytes using a transaction ID hash
profile.

Transaction ID must not be computed over JSON, display strings, RPC request
objects, or local memory layouts.

### Validation Before Execution

Nodes must perform cheap validation before expensive execution.

Initial validation stages:

```text
bytes
  -> HNCS decode
  -> size limits
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

- final transaction envelope fields
- nonce width and failed-execution behavior
- final fee model
- final validity window semantics
- access list enforcement model
- initial transaction type registry
- receipt model
- event model
- transaction ID hash profile
- signing payload schema
- multi-signature activation model
- threshold authorization model
- transaction size limits
- mempool admission policy

## Related Specifications

- `docs/specs/core/transaction-format.md`
