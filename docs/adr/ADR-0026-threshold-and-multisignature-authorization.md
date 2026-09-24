# ADR-0026: Threshold And Multisignature Authorization

Status: Proposed

Date: 2026-09-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0003: Address Format
- ADR-0006: Transaction Format
- ADR-0007: State Tree

Supersedes: None

## Context

`docs/specs/core/account-state.md` §4.5 Permission State lists 8
conceptual capabilities (owner control, administration, operation,
viewing/read-only access, voting delegation, spending limits, session
authorization, emergency lock or recovery), none activated: "these
concepts are not activated by this specification. Each permission
feature requires explicit state transition rules before implementation."
Unlike Nonce State and Lifecycle State, which each had one concrete,
narrow driver that let this project decide their storage representation
ahead of full activation, Permission State had no such driver — eight
loosely related capabilities with no natural single entry point. `tx_type
= 0x08` (`permission_update`, ADR-0006's closed registry) has sat
reserved-but-unspecified since ADR-0006's own `tx_type` pass, blocked on
exactly this section.

ADR-0002 (Accepted) already anticipated this decision's shape without
making it. Its "Accepted Initial Direction" requires "exactly one active
signing key [per role] unless the owning object specification defines a
threshold or multisignature rule" — no such rule exists yet, for any
role. Its "Decided: `SignatureEnvelope` concrete field list" dropped
`key_reference` from the concrete envelope shape *specifically because*
no such rule existed, stating plainly: "a future multisignature/
threshold specification reintroducing multiple simultaneously-active
keys per role would need to reintroduce something like `key_reference`
at that point, against a concrete rule, rather than inventing its shape
now against one that doesn't exist." This ADR is that specification, for
one role.

A second, independent driver surfaced this session:
`docs/specs/core/genesis-security.md` (Draft) had to decide whether
custody of the three ADR-0024 genesis accounts is enforced on-chain or
procedurally, and — because no threshold/multisignature rule existed
anywhere — resolved it as purely procedural for v1, explicitly naming "a
dedicated future ADR" as the path to revisit that once a general
threshold/multisignature model exists. This ADR is that path, though it
does not itself change genesis-security.md's own v1 decision (see
Compatibility, below) — adopting it for the genesis accounts specifically
would be a separate, later decision.

ADR-0006's transaction format already provisioned for this without
activating it: `signatures` is a list of `SignatureEnvelope`, not a
single value, sized for exactly this future ("multi-signature and
threshold authorization require explicit account permission rules before
activation — deferred along with account-state.md §4.5"). This ADR is
that activation, scoped narrowly.

## Decision

**Scope: the `account_signing` role only.** This ADR activates threshold/
multisignature authorization for `KeyRole::AccountSigning` (ADR-0002) —
the role an ordinary `transfer`/`stake`/`governance` vote/etc.
transaction from a user-controlled account authorizes through today.
`validator_consensus`, `validator_network`, `governance`,
`bridge_operator`, and `identity_recovery` remain single-key, untouched.
Extending threshold authorization to any of those is separate, later work
with its own consequences the project has not analyzed (a threshold
validator consensus key, for example, would interact with ADR-0012's
signature aggregation math in ways this ADR does not consider). This
scope matches the whitepaper's own named account-level multisig use cases
(§17.10: "corporate accounts, DAO treasuries") rather than the
consensus-role ones the same section also lists.

**Opt-in per account, not universal.** An account with no stored
multisig configuration behaves exactly as it does today: single active
key, resolved via ADR-0002's `active_key(identity, role, height)`
mechanism — now concretely resolved for `account_signing` by ADR-0027
(Identity State And Account Key Bootstrap), written after this ADR but
depended on by it retroactively (see "Explicitly Not Resolved," below,
updated accordingly). Only an account that has sent a
`permission_update` activating this feature gains a threshold
requirement. This is the same lazy-by-default shape Extension State
(§4.8) already uses, and it means the schema growth this ADR introduces
is paid only by accounts that opt in.

### Decided: `PermissionValueV1`

```text
PermissionValueV1
  u16 permission_version = 1
  optional MultisigConfigV1 account_signing_multisig

MultisigConfigV1
  u8 threshold
  list<KeyDescriptorV1> authorized_keys
```

- `account_signing_multisig` absent (the common case, and every
  account's implicit starting state): single-key mode, unchanged from
  today.
- present: authorizing an `account_signing`-role transaction from this
  account requires exactly `threshold` (`M`) of `authorized_keys.len()`
  (`N`) keys to each contribute a valid, distinct signature (see
  "Decided: Multi-signature verification rule," below).
- Invariant: `1 <= threshold <= authorized_keys.len() <=
  MAX_AUTHORIZED_KEYS`. `MAX_AUTHORIZED_KEYS = 16` is an implementation
  resource bound, the same class of decision as `MAX_ACCESS_LIST_ENTRIES
  = 256` (ADR-0006) and `MAX_ASSET_HOLDINGS = 1024` (account-state.md
  §4.7) — generous headroom over any realistic custody committee size,
  not derived from a specific use case, raise later if a real need
  exceeds it.
- `authorized_keys` is an **ordered list, not a set**: unlike
  `AssetValueV1`'s map (account-state.md §4.7), element order here is
  meaningful — it is exactly what `key_reference` indexes into (below) —
  so canonical form is a deterministic sort (ascending by each entry's
  raw `public_key` bytes), duplicates forbidden, rather than an
  order-independent collection. Any client constructing a
  `permission_update` can compute this order independently; it does not
  depend on the order operators happened to list keys in.
- `KeyDescriptorV1` reuses ADR-0002's already-concrete `KeyDescriptor`
  fields for the `account_signing` role (`algorithm_id`, `public_key`) —
  `descriptor_version` is implied by this schema's own
  `permission_version`, not separately repeated, the same redundancy
  check this project has applied repeatedly (`protocol_name`,
  `checksum_profile`, `hash_profile`, `quorum_threshold`,
  `verification_context` were each dropped for the same reason).

Storage location: `PermissionValueV1` fills `SectionId = 0x04`'s
already-reserved-but-placeholder leaf (ADR-0007, account-state.md §4.5).
No new state domain or section is introduced.

### Decided: `key_reference` in `SignatureEnvelope`

```text
SignatureEnvelope
  envelope_version    (bumps to 2 when key_reference is present)
  algorithm_id
  optional u8 key_reference
  signature
```

`key_reference`, when present, is an index into the signer's stored
`authorized_keys` list — **not** an inline public key. Asked explicitly
(genuine fork, both options defensible): an inline 32-byte key would be
fully self-describing at decode time, at the cost of 32 bytes per
signature instead of 1, and both options need the same state lookup at
*verification* time regardless (confirming the referenced key is one of
the account's actual authorized keys) — so the extra self-description an
inline key buys is smaller than it first appears. Index-based also
matches this project's existing convention for referencing a
curated/moderate-size registry entry compactly (`asset_id: u16`,
`extension_id: u16`) rather than inlining the referenced value.

`key_reference` is present **if and only if** the signer's account has an
active `account_signing_multisig` configuration *at the height this
transaction is validated against* — absent otherwise. Decode must reject
any other combination, the same canonical-encoding discipline
`ValidatorUpdatePayloadV1.new_consensus_key`'s presence-matches-operation
rule and `VoteSigningPayloadV1`'s nil-target rule already established:
exactly one valid encoding per semantic state, never left to convention.

`envelope_version` bumps to `2` only for envelopes carrying a present
`key_reference`. `envelope_version = 1` (the already-implemented,
already-oracle-verified three-field shape: `envelope_version`,
`algorithm_id`, `signature`) remains valid and decodable forever for
every single-key-mode signature — this is additive, not a breaking
replacement of the existing shape (ADR-0022, Structure Versioning).

### Decided: Multi-signature verification rule

For a transaction sent by an account with an active
`account_signing_multisig` configuration:

1. Read `PermissionValueV1` at the sender's Permission-section leaf,
   *as of the state this transaction is validated against* (the same
   state-read timing every other precondition check in this project
   already uses).
2. `signatures` (already a list, ADR-0006) must contain at least
   `threshold` entries whose `key_reference` values are distinct and each
   within bounds of `authorized_keys`.
3. Each such entry must independently verify
   (`SignatureEnvelope::verify`) against
   `authorized_keys[key_reference]` over the same `signing_digest` every
   ordinary transaction already uses (ADR-0006's signing payload
   mechanism, entirely unchanged) — there is no separate,
   multisig-specific signing payload.
4. Entries beyond the first `threshold` distinct valid ones are permitted
   but never required — mirrors the already-decided "cap, not exact"
   philosophy (ADR-0006's fee model): a client gathering signatures
   opportunistically does not need to know the exact minimum in advance
   or trim to exactly `threshold` before broadcasting.
5. For an account with no active configuration, verification is exactly
   today's single-signature rule — entirely unchanged, no new state read
   introduced for the common case.

No signature aggregation (e.g. a single combined BLS signature) — this
ADR uses `threshold` separate, independently-verified Ed25519 signatures,
staying inside the already-Accepted "Ed25519 as the only active consensus
signing algorithm" profile (ADR-0002) rather than requiring a second
algorithm.

### Decided: `permission_update` payload (`tx_type = 0x08`)

```text
PermissionUpdatePayloadV1
  u16 payload_version = 1
  MultisigConfigV1 new_account_signing_multisig
```

One operation only — set the account's `account_signing_multisig`
configuration to `new_account_signing_multisig` — not a discriminated
multi-operation payload like `ValidatorUpdatePayloadV1`/
`GovernancePayloadV1`: those needed a discriminant because they each
carry several genuinely different operations sharing one `tx_type` slot;
this pass has exactly one. A discriminant becomes worth adding the moment
a second real operation (see "Explicitly Not Resolved," below) is
decided, not before.

Two cases, distinguished by the sender's *current* stored state, not by
the payload itself:

- **First activation** (no existing `account_signing_multisig`):
  authorization is today's existing single-key rule, unchanged —
  whatever key is currently active for this account under ADR-0002's own
  (still separately deferred) default mechanism must sign this
  transaction, the same dependency every other transaction from this
  account already has today. This ADR does not introduce a new
  dependency here, only reuses the existing one.
- **Reconfiguration** (an `account_signing_multisig` is already active):
  authorization is this ADR's own multi-signature verification rule
  (above), checked against the *pre*-transaction configuration —
  changing the key set or threshold itself requires meeting the
  currently-active threshold. Fully self-contained: uses only this ADR's
  own storage, no Identity-State dependency.

## Explicitly Not Resolved

**Deactivating back to single-key mode is not supported by this ADR.**
An account cannot use `permission_update` to remove its
`account_signing_multisig` configuration and revert to plain single-key
mode. This is a deliberate scope cut, not an oversight: reverting would
require designating which one key becomes "the" account's active
identity key afterward — writing a new `IdentityValueV1` (ADR-0027,
Identity State And Account Key Bootstrap). Account-level key *rotation*
itself is now decided (ADR-0028, Account-Level Key Rotation,
`RotateIdentityKey`) — but deliberately scoped to single-key mode only:
its authorization model is "the account's one current key signs its own
replacement," which is not the right shape for deactivation, where the
*group* under the current multisig threshold — not any single member —
must be the one to agree on a successor key. ADR-0028 states this
explicitly in its own Open Decisions rather than silently leaving the
question to look more resolved than it is. Deactivation therefore still
needs its own mechanism (a third `permission_update` operation,
multisig-threshold-authorized, that both clears
`account_signing_multisig` and writes the chosen successor
`IdentityValueV1` together) — natural, narrow follow-up work, not
blocked on anything this ADR introduces, but not automatically granted
by ADR-0028 either.

The other 7 Permission State capabilities (administration, operation,
viewing/read-only access, voting delegation, spending limits, session
authorization, emergency lock or recovery) remain entirely unaddressed,
exactly as before this ADR. This ADR resolves "owner control" only to the
extent of letting it be threshold-shared instead of single-key; it does
not introduce role-differentiated permissions (e.g. an "operator" key
that can transact but not reconfigure the multisig itself) — every
authorized key in `authorized_keys` has identical authority once it
contributes toward `threshold`.

No timelock or delay on reconfiguration: a threshold-reaching set of
signers can change the key set or threshold immediately, in the same
transaction that reaches threshold. Consistent with
`genesis-security.md`'s own v1 scope choice (no on-chain timelock there
either) rather than contradicting it — see Security Considerations.

## Rejected Options

### Signature Aggregation (BLS Or Similar)

Rejected for this pass: would require accepting a second consensus
signing algorithm beyond Ed25519 (ADR-0002's "Accepted Initial
Direction" names Ed25519 as the only active one), a much larger
cryptographic-profile decision this ADR does not need to make to resolve
`permission_update`. `threshold` separate Ed25519 signatures is simpler
and stays inside the already-accepted profile.

### Inline Public Key Instead Of Indexed `key_reference`

Rejected (asked explicitly, user's decision) — see "Decided:
`key_reference` in `SignatureEnvelope,`" above: both options need the
same state lookup at verification time, and the indexed form matches
this project's existing convention for compact references into a
moderate-size registry (`asset_id`, `extension_id`).

### Universal/Mandatory Multisig For Every Account

Rejected: would give every account, including every account that will
never use this feature, a stored multisig configuration from creation —
unnecessary schema growth, and a needless behavior change for every
existing single-key account. Opt-in matches Extension State's own
lazy-by-default precedent.

### Extending Threshold Authorization To Other `KeyRole`s Now

Rejected for this pass: `validator_consensus` threshold signing in
particular would interact with ADR-0012's vote/QC aggregation math in
ways this ADR has not analyzed, and `governance`/`bridge_operator`/
`identity_recovery` each have their own unexamined consequences. Scoping
to `account_signing` only keeps this decision to the use case that
actually motivated it (corporate/DAO/custody accounts) without silently
assuming the same design fits every other role.

### Supporting Deactivation In This Pass

Rejected — see "Explicitly Not Resolved," above: would require inventing
an answer to a question (single-key identity resolution) this project has
already, separately, left open in ADR-0002.

## Security Considerations

Out-of-bounds or duplicate `key_reference`:

- Risk: a malformed transaction references a nonexistent index, or
  repeats the same index across multiple `signatures` entries to
  simulate more distinct signers than actually participated.
- Mitigation: "Decided: Multi-signature verification rule" (above)
  requires each counted entry's `key_reference` to be both in-bounds and
  distinct from every other counted entry.

Loss of enough authorized keys to fall below threshold:

- Risk: an account loses access to enough of its `authorized_keys` that
  the remaining accessible keys fall below `threshold`, permanently
  locking the account (no protocol-level recovery path).
- Mitigation: none at the protocol level — the same residual risk
  `genesis-security.md`'s own "Custodian loss or unavailability" entry
  already names for procedural custody, now also true for any account
  that opts into this on-chain mechanism. Left as a known, accepted v1
  limitation, not hidden.

Immediate, undelayed reconfiguration:

- Risk: a threshold-reaching set of signers (whether legitimately
  authorized or having reached threshold through compromise/collusion)
  can change the key set or threshold in the same transaction that meets
  the old threshold, with no delay for other stakeholders to notice or
  react.
- Mitigation: none in this pass — consistent with, not weaker than,
  `genesis-security.md`'s own v1 choice to skip on-chain timelocks
  entirely. A future ADR could add an optional reconfiguration delay
  without breaking this one's wire shape (an additive field), the same
  pattern `ValidityWindowV1`/`PendingUnbondingV1` already establish for
  height-gated effects.

`MAX_AUTHORIZED_KEYS` exhaustion:

- Risk: an unbounded `authorized_keys` list makes verification cost
  (and state leaf size) attacker-influenced without limit.
- Mitigation: `MAX_AUTHORIZED_KEYS = 16` bound (Decided, above), the same
  DoS-bound class as every other bounded collection in this project.

Silent role confusion:

- Risk: a reader assumes activating `account_signing_multisig` also
  protects other roles (`validator_consensus`, `governance`, etc.) on the
  same account.
- Mitigation: "Scope: the `account_signing` role only" (Decided, above)
  is stated as the very first line of the Decision section, not buried.

## Compatibility

Additive only: a new optional section-value field
(`account_signing_multisig`), a new optional envelope field
(`key_reference`, gated on a version bump that coexists with the
existing shape), and a payload for an already-reserved-but-unspecified
`tx_type` (`0x08`). No existing account's behavior changes unless that
account explicitly opts in via `permission_update`. This ADR does not
change `docs/specs/core/genesis-security.md`'s own v1 decision (procedural
custody, no on-chain enforcement) — the three genesis accounts remain
procedurally managed unless and until a separate, later decision opts
them into this mechanism instead.

## Open Decisions

- account-level key rotation — **resolved, ADR-0028**, but scoped to
  single-key mode only (rejected while multisig is active); does not by
  itself resolve deactivation, below
- deactivation back to single-key mode — still open: needs its own
  multisig-threshold-authorized operation (a distinct authorization
  shape from ADR-0028's single-key-signs-its-own-replacement model),
  not granted by ADR-0028 despite the surface similarity
- the 7 remaining Permission State capabilities this ADR does not touch:
  administration, operation, viewing/read-only access, voting delegation,
  spending limits, session authorization, emergency lock or recovery
- role-differentiated authority within a multisig configuration (e.g. an
  operator key with narrower authority than a full signer)
- optional reconfiguration timelock/delay (see Security Considerations)
- extending threshold authorization to `validator_consensus`,
  `governance`, `bridge_operator`, or `identity_recovery` roles
- whether the ADR-0024 genesis accounts ever adopt this mechanism instead
  of `genesis-security.md`'s procedural model (a separate, later decision
  this ADR only makes possible, not automatic)

## Related Specifications

- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0007-state-tree.md`
- `docs/adr/ADR-0027-identity-state-and-account-key-bootstrap.md`
- `docs/adr/ADR-0028-account-level-key-rotation.md`
- `docs/specs/core/account-state.md`
- `docs/specs/core/genesis-security.md`
- `docs/whitepaper/HNChain-Whitepaper-v0.1-draft.md` (§17.10,
  "Multi-Signature")
