# ADR-0029: Multisig Deactivation

Status: Proposed

Date: 2026-09-24

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0006: Transaction Format
- ADR-0026: Threshold And Multisignature Authorization
- ADR-0027: Identity State And Account Key Bootstrap
- ADR-0028: Account-Level Key Rotation

Supersedes: None

## Context

The one item ADR-0026 left open that ADR-0027 and ADR-0028 each, in
turn, explicitly declined to resolve: reverting an account's active
`account_signing_multisig` configuration back to plain single-key mode.
ADR-0026's own "Explicitly Not Resolved" named the reason at the time —
Identity State had no schema to write a successor key into. ADR-0027
resolved that schema. ADR-0028 then resolved account-level key
*rotation* — but deliberately scoped to single-key mode only, stating
plainly that deactivation "needs multisig-*threshold* authorization ...
a materially different authorization path." This ADR is that path.

The distinction is not cosmetic. ADR-0028's `RotateIdentityKey` is
authorized by "the account's one current key signs its own
replacement" — a single signer, unchanged from ADR-0027's ordinary
resolution. Deactivation instead needs the *current signer group* —
whichever `threshold`-of-`N` combination of `authorized_keys` already
authorizes any other `SetAccountSigningMultisig` reconfiguration
(ADR-0026) — to agree on a successor key together. Reusing
`RotateIdentityKey`'s wire shape or authorization rule for this would
silently substitute single-signer authority where the whole point of
multisig is that no single signer has it.

## Decision

**Decided: a third `permission_update` operation,
`payload_version = 3`.**

```text
PermissionUpdatePayloadV1
  payload_version = 1  =>  SetAccountSigningMultisig(MultisigConfigV1)   (ADR-0026)
  payload_version = 2  =>  RotateIdentityKey(KeyDescriptorV1)            (ADR-0028)
  payload_version = 3  =>  DeactivateMultisig(KeyDescriptorV1)           (this ADR)
```

Same wire shape as `RotateIdentityKey` (a bare `KeyDescriptorV1` — the
group-chosen successor key), but a genuinely distinct `payload_version`
rather than reusing version 2's bytes with state deciding the meaning.
Reusing one encoding for two different state transitions, disambiguated
only by whatever `PermissionValueV1` happens to say at apply time, was
considered and rejected: every `apply_*` function in this crate
determines its transition purely from its own payload, never by reading
state first (the same boundary `apply_stake`/`apply_transfer`/
`apply_permission_update` itself already draw between "authorized" and
"what state results") — collapsing two operations into one wire shape
would break that for the first time, and would fail this project's own
repeated "exactly one canonical encoding per semantic state" rule the
moment `RotateIdentityKey` and `DeactivateMultisig` ever needed to be
told apart without also fetching state. `payload_version` is cheap and
already established as this payload's own discriminant (ADR-0028) — a
third value costs nothing new.

**Decided: authorization is `verify_multisig_authorization`, unchanged
— no new verification code.** A `DeactivateMultisig` transaction is
authorized exactly like a `SetAccountSigningMultisig` reconfiguration:
the sender's *pre*-transaction `MultisigConfigV1` must be met
(`threshold`-of-`N` distinct valid signatures). `TransactionEnvelope::
verify` (ADR-0027) already dispatches to `verify_multisig_authorization`
for *any* `permission_update` payload whenever `account_signing_multisig`
is active, regardless of which specific operation the payload carries —
this ADR needs no changes there at all, only a new state-transition
branch for what a successful deactivation writes.

**Decided: the successor key needs no address-derivation check and no
membership requirement.** Unlike `bootstrap_key` (ADR-0027), the
successor key does not need to derive the account's own address — the
address was fixed at genesis/creation by whichever key originally
bootstrapped it (ADR-0028's `RotateIdentityKey` already established
this precedent for account-level key changes generally: a later key
never needs to re-derive an address that was fixed before it existed).
The successor is also not required to already be one of the outgoing
`authorized_keys` — the group may hand control to any key it collectively
agrees on, including one not previously part of the multisig set.

**Decided: writes two leaves, across two sections, in one operation —
`apply_permission_update`'s return type changes from `[Leaf; 1]` to
`Vec<Leaf>`.** A successful `DeactivateMultisig` clears `sender`'s
Permission-section leaf (`PermissionValueV1.account_signing_multisig =
None`, an unconditional overwrite — clearing does not need to read the
old configuration first, only replace it) *and* sets the Identity-
section leaf (`IdentityValueV1 { key: successor }`) in the same state
transition, mirroring the same "one transaction, several leaves as an
automatic side effect" pattern `transfer`'s own implicit account
creation and `permission_update`'s own first-activation bootstrap case
already established. This is a genuine, deliberate departure from this
crate's otherwise-consistent "leaf count is fixed per function, encoded
in the return type" convention (`apply_transfer -> [Leaf; 2]`,
`apply_stake -> [Leaf; 1]`, `apply_vote -> [Leaf; 2]`, every branch of
`apply_validator_update`'s own 5-operation match agreeing on `[Leaf;
1]`): `permission_update`'s three operations are not actually uniform
in how many sections they touch — `SetAccountSigningMultisig`/
`RotateIdentityKey` each touch exactly one, `DeactivateMultisig` touches
two — and forcing that non-uniformity through a fixed-size array would
need either an artificial padding leaf or a second entry-point function,
both worse than acknowledging the real shape. `apply_permission_update`
remains the one entry point for every `permission_update` operation,
per `apply_validator_update`'s own "one entry point per `tx_type`"
precedent — only its container type changes to fit.

## Rejected Options

### Reusing `RotateIdentityKey`'s Wire Shape, State-Disambiguated

Rejected — see "Decided: a third operation," above: would make
`apply_permission_update` read state to know which transition to
perform, breaking every `apply_*` function's shared "payload alone
determines the transition" boundary, and would violate this project's
own repeated canonical-encoding discipline.

### A Padding Leaf To Keep `[Leaf; 2]` Uniform Across All Three Operations

Considered as a way to avoid changing `apply_permission_update`'s
return type at all (always return 2 leaves; `SetAccountSigningMultisig`/
`RotateIdentityKey` would re-write their own single leaf's *unchanged*
value a second time as a no-op filler). Rejected: a write-set entry
that writes a leaf to its own already-current value is not a no-op at
the state-tree level — it still touches the leaf, still needs hashing
and inclusion in the write set, for zero actual effect. Manufacturing
busywork to preserve a fixed array size is worse than an honest variable
return type.

### Requiring The Successor Key To Already Be An Authorized Signer

Rejected: no security or correctness reason requires it, and it would
foreclose a legitimate use case (a multisig group handing control to a
freshly generated, previously-unused key as part of deactivating).

## Security Considerations

Threshold-authorized handoff to an attacker-controlled key:

- Risk: a compromised or colluding threshold-sized subset of the
  current signer group deactivates multisig in favor of a key they
  control.
- Mitigation: not a new risk this ADR introduces — it is exactly the
  same risk `SetAccountSigningMultisig` reconfiguration already carries
  (a threshold-meeting group can already change the authorized key set
  arbitrarily), now also reachable via one additional operation with an
  identical authorization bar. No weaker than what already existed.

No timelock on deactivation:

- Risk: deactivation takes effect immediately, the same as every other
  `permission_update`/`RotateIdentityKey` operation this session has
  decided — no delay for other stakeholders to notice and react.
- Mitigation: none in this pass — consistent with, not weaker than,
  every other reconfiguration-shaped decision already made
  (`genesis-security.md`'s own v1 choice, ADR-0026's reconfiguration
  rule, ADR-0028's rotation timing) — not a new gap this ADR opens.

Successor key not a current signer:

- Risk: the group deactivates in favor of a key none of the outgoing
  signers can independently verify was correctly generated (no
  address-derivation check ties it to anything).
- Mitigation: procedural, not protocol-level — the same class of
  residual risk `genesis-security.md` already documents for off-chain
  custody ceremonies generally; verifying a chosen successor key is
  legitimate before authorizing deactivation is the signer group's own
  operational responsibility, the same as choosing that key correctly
  in the first place.

## Compatibility

Additive to `PermissionUpdatePayloadV1`: `payload_version` values 1 and
2 are completely unchanged. `apply_permission_update`'s return type
change (`[Leaf; 1]` to `Vec<Leaf>`) is a Rust API change, not a wire
change — nothing calls it from outside this crate yet (no
block-processing pipeline exists anywhere in this codebase), so there
is no external breakage.

## Open Decisions

- whether a future operation lets a group deactivate directly into a
  *new* multisig configuration in one step, instead of always landing
  in single-key mode first — not decided here, deliberately out of
  scope (this ADR resolves exactly the gap ADR-0026 named: reverting to
  single-key mode)
- a timelock/delay on any `permission_update` operation — still
  nowhere decided, consistent with every prior pass in this area

## Related Specifications

- `docs/adr/ADR-0026-threshold-and-multisignature-authorization.md`
- `docs/adr/ADR-0027-identity-state-and-account-key-bootstrap.md`
- `docs/adr/ADR-0028-account-level-key-rotation.md`
