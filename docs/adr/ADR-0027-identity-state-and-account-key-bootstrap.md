# ADR-0027: Identity State And Account Key Bootstrap

Status: Proposed

Date: 2026-09-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0003: Address Format
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0007: State Tree

Supersedes: None

## Context

A real, previously-unnamed gap: **no mechanism anywhere in this project
lets a node obtain an ordinary account's Ed25519 public key in order to
verify that account's very first transaction.** This was found while
looking for the second (and third) occurrence of a blocker this session
kept naming without resolving — ADR-0002's own `active_key(identity,
role, height)` state lookup mechanism, listed as a Deferred Decision
since ADR-0002 itself, re-surfaced in `account-state.md` §10 while
writing ADR-0026, and named again by ADR-0026's own "Explicitly Not
Resolved" section as the reason account-level multisig deactivation
could not be decided. All three pointed at the same missing piece:
Identity State (`SectionId 0x01`, account-state.md §3.2/§4.1) has no
decided value schema anywhere, despite `identity_version` already having
a reserved slot in `SectionVersionsV1`.

Investigating *why* this matters beyond "a missing schema" surfaces a
sharper problem, not written down anywhere before this ADR. Three
already-accepted decisions combine into a bootstrapping contradiction:

1. Ed25519 (ADR-0002, the only active consensus signing algorithm) does
   **not** support recovering a public key from a signature — unlike
   ECDSA/secp256k1, which Ethereum-style implicit account models rely on
   for exactly this purpose.
2. An account's on-chain `address` is a one-way hash of its public key
   and other fields (`hn_crypto::account_address_body`, ADR-0003) — the
   public key cannot be recovered from the address either.
3. Account creation is implicit (ADR-0006, "Decided: implicit account
   creation," explicitly modeled on Ethereum rather than Solana's
   explicit-registration style) — an account may send its first
   transaction with no prior on-chain registration step at all.

Put together: a brand-new account's first transaction supplies neither
its public key (nothing in `TransactionEnvelope`/`SignatureEnvelope`
carries one — `sender` is only the hashed `address_body`, and
`SignatureEnvelope` carries only `algorithm_id`/`key_reference`/
`signature`) nor can the node derive it from anything it does have.
There is no state to look up yet (nothing has ever been written for
this account's Identity section), and no way to recover the key from
the signature itself. `ADR-0002`'s `active_key(identity, role, height)`
concept assumes the key it looks up already exists in state — it does
not, by construction, for a never-transacted account. Validators do not
have this problem: `validator_register` is an *explicit* operation that
supplies `new_consensus_key` inline, giving `hn_state::active_key`
something concrete to read from `ValidatorRecordV1.consensus_key`
(already implemented, `hn-state/src/active_set.rs`). Ordinary accounts
have no equivalent explicit step, by design (implicit creation).

This ADR resolves both the general Identity State schema (unblocking
ADR-0026's own deactivation question and ADR-0002's Deferred Decision
generally) and this specific bootstrap contradiction.

## Decision

**Decided: `IdentityValueV1` (Identity State, `SectionId 0x01`).**

```text
IdentityValueV1
  u16 identity_version = 1
  u16 algorithm_id
  bytes public_key   (bounded, hn_crypto::PUBLIC_KEY_MAX_LEN)
```

Represents the account's currently-active `account_signing` key —
`algorithm_id`/`public_key` mirror `hn_crypto::KeyDescriptor`'s own
concrete fields exactly (the same shape ADR-0026's
`MultisigConfigV1.authorized_keys` entries already use), reusing
`hn_state::key_descriptor::{encode_key_descriptor, decode_key_descriptor}`
(parameterized `KeyRole::AccountSigning`) rather than inventing a second
encoding for the same kind of value. Fills the already-reserved
`SectionId 0x01` leaf (ADR-0007, account-state.md §4.1
`SectionVersionsV1.identity_version`).

**Decided: bootstrap mechanism — supplied once, lazily, stored from
then on.** Chosen over two rejected alternatives (below) specifically
because it is the only option consistent with both already-accepted
decisions at once: implicit account creation (ADR-0006) stays implicit
— no separate registration transaction — and the public key is not
repeated on every future transaction forever, leaving room for a future
account-level key rotation decision to have somewhere real to write
(a distinct, separate decision this ADR does not make — see Open
Decisions).

`TransactionEnvelope` gains one new field:

```text
TransactionEnvelope
  tx_version
  chain_id
  network_id
  tx_type
  sender
  bootstrap_key      <-- new
  nonce
  fee_limit
  validity_window
  access_list
  payload
  signatures

BootstrapKeyV1  (= KeyDescriptorV1, ADR-0026's own wire shape)
  algorithm_id
  public_key
```

`bootstrap_key` is present **if and only if** `sender` has no stored
`IdentityValueV1` as of the height this transaction is validated
against — absent otherwise. This is a validation-time rule, not a
decode-time one (the same boundary `SignatureEnvelope.key_reference`'s
presence rule already draws, ADR-0026): decoding the envelope does not
require a state read, only *validating* one does. Exactly one canonical
encoding per semantic state, the same discipline
`ValidatorUpdatePayloadV1.new_consensus_key`/`SignatureEnvelope.
key_reference` already established — `bootstrap_key` present when
Identity State already exists, or absent when it does not, are both
rejected, not silently tolerated as harmless redundancy or an
unrecoverable gap.

`bootstrap_key` is an ordinary field of `TransactionSigningPayload`
too — no new carve-out to the already-decided "`TransactionSigningPayload`
mirrors `TransactionEnvelope` minus `signatures`" rule (ADR-0006,
"Signing Payload"). Unlike `signatures`, including `bootstrap_key` in
what gets signed creates no circularity, and doing so directly binds
the specific key bytes into the signed payload — a belt-and-suspenders
addition on top of the address-derivation check below, not required for
soundness by itself (an attacker substituting a different keypair would
also need that keypair to derive `sender`'s exact address, which
preimage resistance already prevents) but adds no cost either.

**Decided: verification/bootstrap procedure.**

For a transaction whose `sender` has an existing `IdentityValueV1`:
`bootstrap_key` must be absent; the `account_signing` role's active key
resolves exactly as ADR-0002's `active_key(identity, role, height)`
concept always intended — read `IdentityValueV1` (or, if ADR-0026's
`account_signing_multisig` is active, resolve via `key_reference`
against `MultisigConfigV1.authorized_keys` instead, unchanged from
ADR-0026). This is the first concrete resolution of `active_key(...)`
for the `account_signing` role — previously conceptual only.

For a transaction whose `sender` has no `IdentityValueV1` yet:
`bootstrap_key` must be present. The node must:

1. Recompute `hn_crypto::account_address_body` over
   `bootstrap_key.algorithm_id`/`bootstrap_key.public_key` (plus the
   transaction's own `network_id`, ADR-0003's other already-decided
   inputs) and reject if the result does not equal `sender` — the
   anti-spoofing check: nobody may claim an address that is not the hash
   of the key they are presenting.
2. Verify the transaction's signature against `bootstrap_key` (ordinary
   `SignatureEnvelope::verify`, `key_reference` absent — this is
   necessarily single-key mode, since ADR-0026 multisig activation
   itself needs a resolvable key to authorize its own first
   `permission_update`, the same bootstrap dependency, see below).
3. If both pass, write `IdentityValueV1 { algorithm_id, public_key }`
   from `bootstrap_key` to `sender`'s Identity leaf — an automatic
   side effect of this transaction succeeding, not a separate operation,
   the same "implicit creation writes several leaves at once" pattern
   `transfer`'s own implicit-account-creation path (ADR-0006, "Decided:
   implicit account creation") already established for Envelope/Nonce/
   Balance/Asset/Lifecycle.

**Decided: interaction with implicit account creation.** This closes
the exact gap ADR-0006's own implicit-creation decision left silent:
Identity State's initial value for a newly-created account is now
decided, the same way Envelope/Nonce/Balance/Asset/Lifecycle's initial
values already were. `account-state.md`'s own still-open item ("newly-
created accounts' Metadata initial value") is unaffected — Metadata
stays open, Identity no longer is.

**Decided: interaction with ADR-0026 (multisig activation).** A first-
ever `permission_update` from an account that has never transacted
before faces the identical bootstrap need: activating multisig still
requires "today's existing single-key rule" to authorize it
(ADR-0026, "Decided: `permission_update` payload"), which itself needs
a resolvable key. Such a transaction carries `bootstrap_key` exactly
like any other first transaction from that account — no special case
for `permission_update` specifically. `IdentityValueV1` is written
first (per the procedure above), then `PermissionValueV1.
account_signing_multisig` is set per ADR-0026 — both as effects of the
same one transaction.

## Rejected Options

### Always Inline, No Stored Identity State

Every transaction carries its sender's public key, forever; no
`IdentityValueV1` leaf, no Identity State schema. Rejected: forecloses
any future account-level key rotation entirely (the address is
permanently tied to one key with nowhere else for a "current key" to
live), and adds fixed per-transaction overhead (`algorithm_id` +
public key bytes) to every single transaction forever, a meaningful
relative cost for small transfers, not a one-time cost paid once per
account.

### Explicit Registration Transaction

Require a dedicated registration operation (mirroring
`validator_register`) before an account may send any other transaction
— Solana-style. Rejected: directly reopens and contradicts ADR-0006's
own already-accepted "Decided: implicit account creation," which chose
implicit/Ethereum-style specifically to avoid this — reversing that
decision is a much larger-blast-radius change than this ADR's own
scope, and no new information has surfaced that would justify revisiting
it.

## Security Considerations

Address/key mismatch (impersonation attempt):

- Risk: a transaction presents a `bootstrap_key` that does not actually
  belong to the claimed `sender` address.
- Mitigation: the address-derivation check (step 1 of the verification
  procedure) rejects any `bootstrap_key` that does not hash to `sender`
  under ADR-0003's own derivation function — relies on
  `HASH_PROFILE_0x0001`'s preimage resistance (ADR-0005), the same
  security assumption every other address-derived check in this project
  already depends on.

Redundant or missing `bootstrap_key`:

- Risk: a client sends `bootstrap_key` when it is not needed (Identity
  State already exists), or omits it when it is needed, either by bug
  or by attempting to exploit ambiguity.
- Mitigation: "Decided: bootstrap mechanism," above — both cases are
  hard validation failures, not tolerated as harmless or silently
  worked around.

`IdentityValueV1` write race across concurrent transactions:

- Risk: two transactions from the same never-before-seen `sender`,
  both carrying a `bootstrap_key`, are processed concurrently.
- Mitigation: not a new risk class — this is the same same-sender
  serialization guarantee the already-decided nonce model provides
  (ADR-0006, "Nonce": strictly increasing, gap-free per sender), which
  already requires same-sender transactions to be ordered; no additional
  concurrency control is introduced by this ADR.

## Compatibility

Additive to `TransactionEnvelope`'s own conceptual field list — as with
ADR-0026's `key_reference`, this refined an already-decided-but-not-
yet-implemented structure at the time this ADR was written.
`TransactionEnvelope`/`TransactionSigningPayload` are now implemented
(`hn_state::transaction_envelope`, a direct follow-up to this ADR — see
its own crate-level documentation), with `bootstrap_key` included from
the start, so this was not a breaking change to any shipped wire bytes.
`IdentityValueV1` and the bootstrap resolution procedure are now
implemented too (`hn_state::identity_value`/`hn_state::
identity_transition`): `IdentityValueV1` fills the previously-
placeholder `SectionId 0x01` leaf with no prior consensus-relevant
content to preserve, so this was not a breaking change there either.
`resolve_account_signing_key`/`apply_identity_bootstrap` implement
this ADR's own "Decided: verification/bootstrap procedure" (steps 1 and
3 — the address-derivation check and the write-on-success side effect);
step 2 (verifying the transaction's signature against the resolved key)
reuses the already-implemented `SignatureEnvelope::verify` directly,
needing no new code.

## Open Decisions

- account-level key rotation (changing `IdentityValueV1` after it is
  first written) — **resolved, ADR-0028 (Account-Level Key Rotation)**:
  immediate effect, no delay (unlike ADR-0010's validator consensus
  key rotation, unrelated and unaffected by either decision), rejected
  while a multisig configuration is active. Multisig *deactivation*
  (picking a successor key when `account_signing_multisig` is removed)
  remains separately open — ADR-0028's own Open Decisions, not resolved
  by it despite the surface similarity
- a `TransactionEnvelope::verify()` composing `resolve_account_signing_key`
  (this ADR) with `verify_multisig_authorization` (ADR-0026) and actual
  signature verification into one call — not yet implemented; every
  underlying primitive it would call now exists
  (`hn_state::identity_transition`/`hn_state::permission_transition`),
  but nothing yet composes them, the same "no block-processing pipeline
  exists yet" situation most of this crate's own state-transition
  primitives are already in
- whether a future post-quantum or alternate algorithm profile changes
  `IdentityValueV1`'s bound `public_key` length assumptions (already
  algorithm-agile via `PUBLIC_KEY_MAX_LEN`, ADR-0002's own reserved
  algorithm identifiers) — not expected to need revisiting, not
  verified here

## Related Specifications

- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0003-address-format.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0026-threshold-and-multisignature-authorization.md`
- `docs/specs/core/account-state.md`
- `docs/specs/core/transaction-format.md`
