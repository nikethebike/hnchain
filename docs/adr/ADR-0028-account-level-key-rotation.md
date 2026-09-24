# ADR-0028: Account-Level Key Rotation

Status: Proposed

Date: 2026-09-24

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0006: Transaction Format
- ADR-0022: Protocol Versioning
- ADR-0026: Threshold And Multisignature Authorization
- ADR-0027: Identity State And Account Key Bootstrap

Supersedes: None

## Context

ADR-0027 (Identity State And Account Key Bootstrap) resolved how
`IdentityValueV1` gets its *first* value but explicitly left changing
an already-populated one open, in its own words: "account-level key
rotation (changing `IdentityValueV1` after it is first written) — a
distinct future decision this ADR does not make." This ADR is that
decision.

Unlike validator consensus key rotation (ADR-0010, "Key Rotation" —
reuses the epoch-boundary admission/deactivation delay, old key
invalidated immediately at the next epoch boundary), account-level
rotation is not a case of copying that mechanism over: validators need
a delay specifically because of round-based consensus message
concurrency (an equivocation-evidence and in-flight-vote ambiguity
concern unique to the consensus protocol). Ordinary account
transactions have no equivalent hazard — same-sender transactions are
already strictly ordered by nonce (ADR-0006, "Nonce": "strictly
increasing, gap-free per sender"), so there is nothing concurrent for a
delay to disambiguate.

`tx_type` (ADR-0006) is closed for `tx_version = 1` — there is no free
registry slot for a new `account_key_rotation` type. `permission_update`
(`tx_type = 0x08`) already touches Identity State as a side effect (its
own first-activation case bootstraps `IdentityValueV1` before setting
`PermissionValueV1`, ADR-0027) — the natural, minimal-friction place
to add a second operation, exactly the moment ADR-0026's own text
anticipated: "a discriminant becomes worth adding the moment a second
real operation is decided, not before."

## Decision

**Scope: single-key mode only.** This ADR resolves rotation for an
account with **no active `account_signing_multisig` configuration**
(ADR-0026). Multisig *deactivation* — designating which key an account
reverts to when its multisig configuration is removed — is **not**
resolved by this ADR, despite looking superficially similar: it needs
multisig-*threshold* authorization (the current group agreeing on a
successor key), a materially different authorization path from this
ADR's single-existing-key-authorizes-its-own-replacement model. See
Open Decisions.

**Decided: rotation activates immediately, no delay.** A rotation
transaction's new key becomes the account's active `account_signing`
key as soon as that transaction is included — no epoch-boundary or
block-count delay, unlike ADR-0010's validator case. Justified by the
absence of the hazard that motivated the validator delay (above), not
by "no reason found to add one": nonce ordering already gives every
account's own transaction sequence a single, unambiguous point at which
the new key starts applying (the next transaction at the next nonce),
the same clarity an epoch boundary gives validators for a different
reason.

**Decided: the old key is invalidated immediately**, the same
"no dual-validity window" choice ADR-0010 made for validators, for the
matching reason: a signature under the old key after the rotation
transaction is unambiguously invalid, not merely stale, simplifying
verification (`resolve_account_signing_key`, ADR-0027, already always
returns exactly one currently-stored key — this ADR does not change
that function's own shape, only what gets written to the state it
reads).

**Decided: rejected while multisig is active.** A `RotateIdentityKey`
operation (below) targeting an account whose `account_signing_multisig`
is currently `Some` is invalid. While multisig is active,
`IdentityValueV1` is not consulted for verification at all (ADR-0026's
own verification rule checks `PermissionValueV1` first) — rotating a
key nobody is checking would be either a meaningless no-op or, worse, a
source of future ambiguity (if multisig is later deactivated, would
deactivation honor the rotated key or the one active when multisig was
first turned on?). Rejecting the combination outright avoids the
question rather than answering it awkwardly.

**Decided: `PermissionUpdatePayloadV1` becomes a two-operation type,
version-discriminated rather than field-discriminated** — `payload_version`
itself distinguishes the operation, no separate operation byte:

```text
PermissionUpdatePayloadV1
  payload_version = 1  => SetAccountSigningMultisig(MultisigConfigV1)   (ADR-0026, wire-unchanged)
  payload_version = 2  => RotateIdentityKey(KeyDescriptorV1)            (this ADR)
```

Mirrors `SignatureEnvelope`'s own `envelope_version` 1-vs-2 split
(ADR-0026) exactly: version 1's wire bytes are untouched (still a bare
`MultisigConfigV1`, no discriminant prefix — ADR-0026's own
already-oracle-verified test vectors keep passing unmodified), and
version 2 is a new, equally bare `KeyDescriptorV1` (reuses
`hn_state::key_descriptor`'s already-shared encode/decode, parameterized
`KeyRole::AccountSigning` — the same wire shape `IdentityValueV1`/
`MultisigConfigV1.authorized_keys` entries already use). No standalone
operation-discriminant byte is added on top of the version number: the
version already disambiguates fully, and adding a second, redundant
discriminant would repeat the same class of redundancy this project has
flagged and removed repeatedly elsewhere (`protocol_name`,
`checksum_profile`, `hash_profile`, ...). A third operation, if one is
ever decided, gets `payload_version = 3` under the same rule.

**Decided: authorization is unchanged, not new code.** A rotation
transaction is authorized exactly like any other transaction from an
account that already has a populated `IdentityValueV1` — signed by the
account's *current* key, resolved by ADR-0027's own
`resolve_account_signing_key(existing_identity: Some(old), bootstrap_key:
None, ...)`, which already returns the old key unchanged by this ADR.
The new key signs nothing until it is itself the current key for some
later transaction — there is no bootstrapping problem here the way
`IdentityValueV1`'s very first population had, since an already-
populated Identity State always has a key to verify against.

**State transition**: `apply_identity_rotation(sender, new_key) ->
Leaf`, structurally identical to `apply_identity_bootstrap`'s own leaf
construction (both just write `IdentityValueV1 { key }` to the
Identity-section leaf) — kept as a separate, intent-revealing name
rather than one generic function, so a reader at either call site
immediately knows which flow they are in, sharing the same private
leaf-builder underneath rather than duplicating it.
`apply_permission_update` dispatches to it for the `RotateIdentityKey`
case, the same "one entry point per `tx_type`, internal match over
operations" shape `apply_validator_update` already uses for 5
operations.

## Rejected Options

### Epoch-Boundary (Or Any Other) Delay

Rejected — see "Decided: rotation activates immediately," above: the
concurrency hazard that justifies a delay for validators does not exist
for nonce-ordered account transactions, so copying the validator
mechanism would add complexity with no matching problem to solve.

### Reusing `SetAccountSigningMultisig` With A Single-Key, Threshold-1 Config

Rejected: would silently switch the account into multisig's own
verification path (`PermissionValueV1.account_signing_multisig`
checked first, `key_reference` required on every future signature,
counted against `MAX_AUTHORIZED_KEYS`) even for an account whose owner
only wanted to replace one key while staying in ordinary single-key
mode — a real, surprising behavior change a plain "rotate my key"
request should not cause.

### A Standalone Operation-Discriminant Byte Alongside The Version

Rejected as redundant: `payload_version` already fully determines which
operation a given payload represents (1:1, by construction — see
"Decided," above), so a second field encoding the same information
would duplicate it for no benefit, the same redundancy class this
project has repeatedly found and removed elsewhere.

## Security Considerations

Old-key signature after rotation:

- Risk: a signature produced by the old key is presented after a
  rotation transaction has been included.
- Mitigation: "Decided: the old key is invalidated immediately," above
  — `resolve_account_signing_key` only ever returns the *currently*
  stored key, so an old-key signature simply fails to verify once
  rotation has taken effect.

Rotation racing other pending transactions:

- Risk: a client has other transactions queued at higher nonces, signed
  with the old key, when a rotation transaction lands.
- Mitigation: not a protocol correctness issue — nonce ordering already
  requires strict sequencing, so those queued transactions would fail
  ordinary signature verification once the key changes underneath them,
  the same way any nonce-sequenced operation that changes a later
  precondition already behaves. A wallet/client UX concern, not a
  consensus one; not mitigated by this ADR because it does not need to
  be.

Rotation combined with multisig:

- Risk: ambiguity about which key is authoritative if rotation were
  allowed while multisig is active.
- Mitigation: "Decided: rejected while multisig is active," above —
  the combination is rejected outright rather than given a specific,
  possibly-surprising resolution.

## Compatibility

Additive to `PermissionUpdatePayloadV1`'s own wire shape: `payload_version
= 1`'s bytes are completely unchanged (still the bare `MultisigConfigV1`
ADR-0026 already implemented and oracle-tested), `payload_version = 2`
is new. Not a breaking change to any already-shipped encoding.

## Open Decisions

- multisig deactivation (designating a successor key when
  `account_signing_multisig` is cleared) — explicitly **not** resolved
  by this ADR despite the surface similarity; **resolved separately,
  ADR-0029 (Multisig Deactivation)**, via its own
  multisig-threshold-authorized operation
- rotating a `validator_consensus` key is ADR-0010's own, separately
  decided mechanism — unrelated, unaffected by this ADR
- whether a rotated-to key may collide with another account's key (key
  reuse across accounts) — not checked here, the same as it is not
  checked at bootstrap time (ADR-0027); addresses are derived from keys
  1:1, so two accounts sharing a key is a user/wallet-hygiene concern,
  not a protocol invariant this project enforces

## Related Specifications

- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0010-validator-set-model.md` (validator consensus key
  rotation — a distinct, separately-decided mechanism)
- `docs/adr/ADR-0026-threshold-and-multisignature-authorization.md`
- `docs/adr/ADR-0027-identity-state-and-account-key-bootstrap.md`
- `docs/adr/ADR-0029-multisig-deactivation.md` (resolves multisig
  deactivation, which this ADR names but does not resolve)
