# HNChain Core Specification: Account State

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## 1. Scope

This document specifies the conceptual account state model for HNChain.

It defines account boundaries, versioning principles, deterministic transition
requirements, extension rules, and module interaction boundaries.

This document does not specify binary encoding, storage column layout,
cryptographic algorithms, transaction format, fee model, VM execution, or
consensus rules. Those must be defined in separate specifications.

This specification is constrained by
`docs/adr/ADR-0000-protocol-invariants.md`,
`docs/adr/ADR-0001-account-state-model.md`,
`docs/adr/ADR-0002-cryptographic-identity.md`, and
`docs/adr/ADR-0007-state-tree.md`.

## 2. Design Goals

- Account state is a first-class protocol object.
- All account changes are deterministic.
- Account structure is extensible without implicit behavior.
- Serialization and hashing are canonical.
- Unknown fields and extensions have defined compatibility behavior.
- Storage, execution, RPC, wallet, and explorer modules depend on interfaces,
  not concrete account implementations.

## 3. Account Object

An account is identified by an address and represented by a versioned account
state object.

Conceptual structure:

```text
AccountState
  envelope_version
  account_type
  address
  identity
  balance
  nonce
  permission
  metadata
  asset
  extension
  lifecycle
```

Each section has its own version and validation rules.

`identity` through `lifecycle` above are named to match ADR-0007's SectionId
registry exactly (`envelope`, `identity`, `balance`, `nonce`, `permission`,
`metadata`, `asset`, `lifecycle` — §4.1-4.9 below use the same singular
names), except `extension`: it is deliberately not part of that registry.
ADR-0007 addresses it through the separate `account_extensions` domain
instead (§4.8), because its cardinality and payload size differ from the
fixed core sections.

`address` is an `AddressPayload` (ADR-0003) with `address_namespace = 0x01`
(`account`).

The canonical binary representation must be defined in the serialization
specification before implementation.

## 3.1 Account Types

**Decided: segregated model.** `account_type` distinguishes kinds of object
*within* the `accounts` domain (ADR-0003 `address_namespace = 0x01`,
ADR-0007 state `domain_id = 0x0001`) only. It is not a general-purpose tag
for every kind of protocol object.

`account_type` is `u8`, a closed registry (Envelope, §4.1):

```text
0x00  reserved, invalid
0x01  standard
```

Validators and smart contracts are not `account_type` values: each is a
fully separate category with its own `address_namespace` and state
`domain_id` (ADR-0003 `contract = 0x02` / `validator = 0x03`, ADR-0007
`contracts = 0x0003` / `validators = 0x0006`), not an `AccountState`
instance distinguished by type. An earlier draft of this list included
`validator`, `smart_contract`, and `system` as account types; that
conflated three independently-built registries — this document's own
(unnumbered) `account_type`, ADR-0003's `address_namespace`, and
ADR-0007's state `domain_id` — which classified the same conceptual space
three different ways and did not fully agree with each other. Contracts
and validators are segregated because ADR-0007 already gives each its own
state domain with its own leaf structure (`contract_storage` alongside
`contracts`, for example), which only makes sense if their state is not
shaped like `AccountState` at all — folding them into `account_type` would
mean pretending they share a structure they were already accepted as not
sharing.

`system` is resolved the same way, not merely deferred: it is not an
`account_type` value either. ADR-0003's `protocol` namespace (covering
treasury, governance, staking, slashing, and bridge registry) maps to the
`governance`, `system`, `validators`, and `bridge` state domains by each
module's own cardinality (ADR-0007, State Domains, "Decided: `protocol`
namespace to domain mapping") — never to `accounts` (`0x0001`). That
mapping holds regardless of which specific domain a given protocol module
lands in, so removing `system` from `account_type` does not depend on
knowing that mapping's details, only on knowing none of its outcomes is
`accounts`. See `docs/adr/ADR-0007-state-tree.md`'s State Domains section
for the current mapping.

Account type is consensus-relevant.

Unknown account types are invalid unless activated by protocol upgrade rules.

Type-specific behavior must be defined by the owning protocol specification.

## 3.2 Identity Binding

An account does not depend on one raw public key.

Accounts bind to cryptographic identity through key descriptors, role bindings,
or identity commitments defined by the cryptographic identity specification —
concretely, ADR-0002's `KeyDescriptor` (Accepted: algorithm-agile, Ed25519
active for `account_signing` at `algorithm_id = 0x0001`).

This allows key rotation, multisignature, session keys, hardware wallets, and
future post-quantum migration without redefining account identity.

## 4. Required Sections

### 4.1 Envelope

The envelope identifies the account state format.

**Decided: envelope value schema.**

```text
EnvelopeValueV1
  u16             envelope_version = 1
  u8              account_type
  bytes32         address
  SectionVersionsV1 section_versions

SectionVersionsV1
  u16 identity_version
  u16 balance_version
  u16 nonce_version
  u16 permission_version
  u16 metadata_version
  u16 asset_version
  u16 lifecycle_version
```

Fields:

- `envelope_version`: `u16`, matching this project's existing version-field
  width convention (`state_key_version`, `address_version`, and so on). A
  Structure Version in ADR-0022's sense: it changes only when the envelope
  shape itself changes, independently of any individual section's own
  version.
- `account_type`: `u8`, a closed registry (`0x00` reserved/invalid,
  `0x01` = `standard`) matching §3.1's segregated-model resolution — see
  below.
- `address`: this account's own 32-byte `address_body`
  (`hn_crypto::account_address_body`'s output), raw bytes, not a full
  `AddressPayload`. This duplicates what was already used to derive this
  leaf's `state_key` (ADR-0007's `object_id`), which is necessary, not
  redundant: `state_key` is a one-way hash, so a reader holding only a
  leaf's key and value cannot recover the address from the key alone.
  Storing it in the value lets a leaf self-describe and lets any reader
  cross-check the value's claimed address against the key it was found at.
- `section_versions`: one `u16` version per section other than the
  envelope itself, in a fixed position per section — not a bounded map.
  §4 is titled "Required Sections": unlike extension records (§4.8,
  explicitly lazy and optional), every core section is mandatory for
  every account, so there is no sparse/missing case to represent and no
  need for map machinery. The envelope's own version is `envelope_version`
  above, not repeated inside `section_versions`.

The envelope is mandatory and must be included in state hashing.

### 4.2 Versioned Storage Record

Every persisted account-related record must have an explicit storage envelope.

Conceptual structure:

```text
StorageRecord
  record_version
  owner
  record_type
  payload
```

Fields:

- `record_version`: version of the storage record envelope.
- `owner`: account address or protocol namespace that owns the record.
- `record_type`: canonical identifier for the contained payload type.
- `payload`: canonical bytes defined by the payload specification.

Storage records describe persistence boundaries. They do not define account
semantics.

### 4.3 Balance State

Balance state represents the account's native HNCOIN balance only. Non-native
asset balances (protocol-level, bridged) are held in §4.7 Asset State instead
— see there for the split rationale.

Balance mutation is allowed only through valid state transitions.

Balance values must use fixed-width unsigned integers. Floating point values are
forbidden in consensus state.

**Decided: balance value schema.**

```text
BalanceValueV1
  u16  balance_version = 1
  u128 native_balance
```

Fields:

- `balance_version`: `u16`, a Structure Version in ADR-0022's sense, matching
  `envelope_version`/`nonce_version`'s width convention.
- `native_balance`: `u128`. Native currency is a singleton per account —
  every account has exactly one native balance, always present — unlike
  non-native assets (§4.7), which form a variable-cardinality collection;
  this singleton-vs-collection test is what splits the two sections' scope
  (the same test already used for the protocol namespace's domain mapping
  in ADR-0007). `u128`, not `u64`, is a storage-width decision only:
  headroom against intermediate-computation overflow (fee/reward
  multiplication and similar), not a supply, allocation, or fee decision —
  those remain owned by a future tokenomics specification and are not
  decided by this schema; this section does not assume or encode any
  particular initial balance.

### 4.4 Nonce State

Nonce state prevents transaction replay and defines account transaction ordering.

The full nonce model must be specified before transaction validation is
implemented. At minimum, the model must define:

- nonce width
- initial nonce
- increment rules
- replay protection domain
- behavior for failed execution

**Decided: nonce storage width only.** This section decides how a nonce is
represented as a stored leaf value — nonce width and initial value — not the
full nonce model above. Replay protection domain, increment timing, ordering
rules, and behavior for failed execution are transaction-validation
semantics owned by ADR-0006 (Transaction Format, "Nonce" — now decided
there, though the ADR overall remains Proposed pending its other open
items); this schema decodes and stores whatever `u64` count that model
produces, without assuming any of those rules itself.

```text
NonceValueV1
  u16 nonce_version = 1
  u64 nonce
```

Fields:

- `nonce_version`: `u16`, a Structure Version in ADR-0022's sense, matching
  `envelope_version`'s width convention.
- `nonce`: `u64`, matching `hn_core::AccountNonce`'s existing width and
  initial value (`0`) — this schema does not introduce a new nonce type, it
  gives the already-implemented `AccountNonce` a canonical on-chain storage
  encoding.

### 4.5 Permission State

Permission state defines account-level authorization capabilities.

Permission changes are consensus-relevant and must be included in state hashing.

Permissions must not depend on wall-clock time unless consensus time semantics
are formally defined.

Conceptual permission capabilities may include:

- owner control
- administration
- operation
- viewing or read-only access
- voting delegation
- spending limits
- session authorization
- emergency lock or recovery

These concepts are not activated by this specification. Each permission feature
requires explicit state transition rules before implementation.

### 4.6 Metadata State

Metadata state contains bounded protocol-level account metadata.

Metadata is not a general-purpose unbounded key-value store. Size limits,
allowed value types, canonical encoding, and fee implications must be specified
before activation.

### 4.7 Asset State

Asset state describes the account's non-native asset holdings: this section
holds each held asset's balance line item, keyed by the owning account. It
does not define the assets themselves. Native HNCOIN is out of scope here —
it is a singleton, held in §4.3 Balance State instead.

The asset model must distinguish between:

- protocol-level assets
- bridged assets
- contract-defined assets

Protocol-level and bridged asset *definitions* (supply, decimals, mint/burn
authority) live in ADR-0007's `assets` state domain (`0x0005`), keyed by a
curated `asset_id`, not in this account section and not behind their own
address (ADR-0003 deliberately has no `asset` address namespace). This
section's balance line items reference those definitions by `asset_id`;
they do not duplicate them. Contract-defined assets are defined, and their
holder balances tracked, in `contract_storage` under the issuing contract's
own address instead (the same place ERC-20-style token balances would live
under an account-based model) — they are not represented in this section at
all, since they are not keyed by the curated `asset_id` registry.

No asset class may be introduced without explicit supply, ownership, and
validation rules.

**Decided: asset value schema (protocol-level and bridged assets only).**

```text
AssetValueV1
  u16 asset_version = 1
  map<u16 asset_id, u128 amount>
```

Fields:

- `asset_version`: `u16`, a Structure Version, matching this account
  section's sibling schemas' width convention.
- the map: a bounded (`MAX_ASSET_HOLDINGS = 1024`), canonically-sorted
  `asset_id -> amount` map (HNCS `map<K, V>`, ADR-0004) — variable
  cardinality is exactly why this is a collection and not a fixed
  positional structure like the envelope's `section_versions`, per the
  singleton-vs-collection split above. Absent from the map means a zero
  balance; there is no explicit zero-amount entry.
- `asset_id`: `u16`, matching the width of other curated/moderate-size
  registries in this codebase (`extension_id`, `hash_profile_id`) rather
  than a permissionless-derivation-sized registry — the `assets` domain's
  `asset_id` is protocol-curated (ADR-0007, ADR-0003), not
  self-assigned.
- `amount`: `u128`, matching `native_balance`'s width for the same
  overflow-headroom reason; not itself an economic decision.

### 4.8 Extension State

Extension state allows protocol evolution.

Every extension has:

- extension identifier
- extension version
- criticality flag
- canonical payload
- activation rules

Unknown critical extensions make the account state invalid for nodes that do not
support them.

Unknown non-critical extensions may be preserved but must not affect consensus
behavior unless supported and activated by protocol rules.

Extension loading is lazy by default.

The core account record contains an extension registry, not necessarily all
extension payloads. Extension payloads are loaded only when required by
validation, execution, indexing, or proof generation.

Lazy loading must not change consensus behavior. A transition that requires an
extension must declare that dependency through canonical transition input or
protocol-defined access rules.

### 4.9 Lifecycle State

Lifecycle state is part of consensus account state.

Conceptual lifecycle:

```text
Created -> Active -> Frozen -> Deprecated -> Archived -> Destroyed
```

Lifecycle meanings:

- `Created`: account exists but may not yet be fully active.
- `Active`: account may participate according to its permissions and balances.
- `Frozen`: account exists but selected transitions are blocked by protocol
  rules.
- `Deprecated`: account is valid but should no longer receive new capabilities
  except migration or archival operations.
- `Archived`: account is no longer on the hot execution path but remains
  provable according to archival rules.
- `Destroyed`: account has completed a protocol-defined removal process.

Lifecycle transitions must define:

- required authorization
- allowed source states
- allowed target states
- balance and asset handling
- metadata and extension handling
- storage record handling
- state root impact
- proof and archival behavior

`Destroyed` must not mean silent deletion unless state tree and historical proof
rules explicitly allow it.

**Decided: lifecycle storage representation only.** This decides how the
current lifecycle state is represented as a stored leaf value — a closed
registry over the 6 states already named above — not the transition list
directly above (required authorization, allowed source/target states,
balance/asset/metadata/extension/storage-record handling, state root
impact, proof/archival behavior), which remains open, owned by a future
account state transition specification (`## 5` below is conceptual only,
not yet a decided rule set).

```text
LifecycleValueV1
  u16 lifecycle_version = 1
  u8  state
```

`state` registry, closed for this profile:

```text
0x00  Created
0x01  Active
0x02  Frozen
0x03  Deprecated
0x04  Archived
0x05  Destroyed
```

Fields:

- `lifecycle_version`: `u16`, a Structure Version, matching this account
  section's sibling schemas' width convention.
- `state`: `u8`. Unlike `account_type` (§3.1), no value is reserved as
  invalid: every one of the 6 named lifecycle states above is a real,
  reachable state, so the registry has no unused sentinel.

## 5. Deterministic State Transitions

All account changes occur through deterministic transition functions:

```text
previous_account_state + valid_transition_input -> next_account_state
```

A transition is valid only if:

- input encoding is canonical
- preconditions are satisfied
- authorization checks pass
- all numeric operations are overflow-safe
- resulting account state passes validation
- resulting state root is deterministic

Nondeterministic inputs are forbidden in consensus transitions, including:

- local system time
- random number generators without consensus-defined randomness
- network response ordering
- floating point arithmetic
- map iteration order without canonical ordering

## 6. Module Boundaries

```text
Transaction Pool
      |
      v
Transaction Validation
      |
      v
State Transition Engine -----> Cryptography
      |
      v
State Database -----------> Storage Engine
      |
      v
State Root / Proof System
      |
      v
Consensus
```

Boundary rules:

- Consensus verifies state roots and transition validity, but must not own
  account storage internals.
- Storage stores canonical bytes and indexed views, but must not define account
  semantics.
- RPC exposes documented account views, but must not bypass state validation.
- Wallets construct transactions, but must not be trusted for state correctness.
- HNVM may request account reads and writes through an execution interface, but
  must not mutate storage directly.

## 7. State Bloat Architecture

HNChain account state must be designed so storage growth can be bounded,
priced, pruned, or archived by future protocol rules.

Required architectural support:

- versioned storage records
- bounded metadata
- lazy extension loading
- explicit account lifecycle
- extension payload size limits
- storage accounting hooks
- compatibility with future rent policy

A rent policy is not activated by this specification. However, the account model
must preserve enough structure to introduce rent, deposits, storage fees,
pruning, or archival incentives without changing the meaning of existing account
fields.

## 8. Compatibility Rules

Account evolution follows SemVer at the specification level.

Patch changes:

- clarify validation rules without changing accepted canonical state
- add non-consensus documentation

Minor changes:

- add optional non-critical sections
- add extension records with deterministic defaults
- add RPC fields that do not change consensus behavior

Major changes:

- change canonical encoding
- change hashing rules
- change required fields
- change transition semantics
- change meaning of an existing field

Major changes require explicit migration and governance activation rules.

## 9. Security Risks

State bloat:

- Risk: metadata and extensions can grow without bound.
- Mitigation: size limits, fees, pruning rules, and storage accounting.

Replay attacks:

- Risk: nonce ambiguity allows transaction replay.
- Mitigation: formally specified nonce domain and signing payload.

Extension confusion:

- Risk: nodes interpret unknown extensions differently.
- Mitigation: criticality flags and activation rules.

Serialization ambiguity:

- Risk: multiple encodings represent the same state.
- Mitigation: canonical binary encoding and rejection of non-canonical forms.

Unauthorized mutation:

- Risk: permissions are bypassed by VM, RPC, or internal modules.
- Mitigation: one state transition engine owns all consensus mutations.

Parallel execution conflicts:

- Risk: concurrent account writes produce nondeterministic results.
- Mitigation: declared access sets, conflict detection, or deterministic
  scheduler rules.

Lazy extension inconsistency:

- Risk: different node classes load different extension payloads and validate
  different effective account state.
- Mitigation: extension dependencies must be declared by protocol rules and
  included in validation, execution, and proof requirements.

## 10. Open Architecture Decisions

The following decisions are required before implementation:

- account address derivation function (namespace, length, and payload
  exclusion are resolved; the exact `HASH_PROFILE_0x0001` inputs are not —
  see below)
- native token unit and numeric width
- nonce model
- canonical serialization format
- storage backend abstraction
- metadata size limits
- extension registry and activation process
- account lifecycle rules
- transaction access-list model

Resolved and removed from this list:

- "state trie or alternative authenticated data structure" is resolved by
  `docs/adr/ADR-0007-state-tree.md` (profile `hn-smt-256-v1`). ADR-0007 also
  defines where each section in the `AccountState` structure above (§3)
  lives in the tree: see its SectionId registry and Account Extensions
  Domain sections.
- "cryptographic identity" is resolved by `docs/adr/ADR-0002-cryptographic-
  identity.md` (Accepted): the `KeyDescriptor`/`SignatureEnvelope` model,
  algorithm-agile with Ed25519 (`algorithm_id = 0x0001`) as the only active
  consensus signing algorithm for `account_signing` at genesis. Key
  rotation transaction semantics and the threshold/multisignature identity
  model remain ADR-0002 Deferred Decisions, not blockers for this item.
- "account address derivation" is partially resolved by
  `docs/adr/ADR-0003-address-format.md` (Accepted): `address_namespace`,
  `address_body` length (32 bytes), and the absence of `chain_id`/
  `checksum_profile` from `AddressPayload` are all decided. Narrowed to
  "account address derivation function" above, since ADR-0003 itself still
  leaves "address body derivation function" open.
