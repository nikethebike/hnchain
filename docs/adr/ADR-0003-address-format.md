# ADR-0003: Address Format

Status: Accepted

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity
- ADR-0005: Hash Algorithms
- ADR-0022: Protocol Versioning

Supersedes: None

Referenced By:

- ADR-0007: State Tree

## Context

HNChain requires addresses for accounts, validators, smart contracts, protocol
modules, bridge objects, identity records, and future namespaces.

Address format depends on cryptographic identity because key algorithms affect
public key encoding, key descriptors, key rotation, signature suites, and
possible post-quantum migration paths.

The address format must support:

- algorithm agility
- network and chain separation
- account type separation
- contract and protocol module addresses
- human-readable encoding with error detection
- canonical binary representation for consensus
- future migration without reinterpreting existing addresses

## Decision

HNChain addresses are versioned protocol identifiers.

Consensus uses a canonical binary `AddressPayload`. Human-readable address
strings are an external representation of that payload.

Conceptual binary structure:

```text
AddressPayload
  address_version
  address_namespace
  network_id
  derivation_scheme
  address_body
```

Conceptual text structure:

```text
hrp + separator + encoded(AddressPayload) + checksum
```

**Decided: no `checksum_profile` in `AddressPayload`.** An earlier draft of
this structure carried `checksum_profile` as a sixth consensus field.
Removed: a text-encoding checksum protects address entry and transport
("The checksum protects address entry and transport. It is not a
cryptographic integrity mechanism," Human-Readable Encoding below) — it
says nothing about what the address identifies. Placing it inside
`AddressPayload` would have made Fixed Consensus Equality below depend on
it, so two payloads identical in every field that actually names an object
(`address_namespace`, `network_id`, `derivation_scheme`, `address_body`)
but encoded with a different checksum algorithm would count as different
addresses — the same class of error as encoding `address_namespace` into
the HRP (decision on HRP scope, above): a display/transport concern bleeding
into consensus identity, just on the opposite side of the payload boundary
this time. See Human-Readable Encoding below for where the checksum
algorithm is actually decided, now that it is not a payload field.

**Decided: no `chain_id` in `AddressPayload`.** `AddressPayload` carries
`network_id` (which environment: mainnet, testnet, a devnet) but not
`chain_id` (which HNChain chain lineage). These are separate fields with
separate purposes elsewhere in the protocol — `docs/specs/core/
transaction-format.md` §4.2 already distinguishes them explicitly
("`chain_id` identifies the HNChain chain," "`network_id` identifies the
network environment") — and only `network_id` belongs in an address.

Addresses are chain-lineage-agnostic by design: the same address (same key
material, namespace, and network) remains valid across a chain split, the
way an Ethereum address is unchanged across ETH and ETC. Replay protection
across a lineage split is a transaction-level concern, already covered by
`chain_id` in `TransactionEnvelope` (ADR-0006 / transaction-format.md §4.2),
not an address-level one. Adding `chain_id` to `AddressPayload` would make
every address split whenever the chain does, with no compensating benefit:
nothing about what an address identifies (a key, a contract, a validator)
changes at a fork.

`chain_id` itself — binary size, type, and value scheme — is not decided by
this ADR. `docs/specs/core/genesis.md` already defines a conceptual
`chain_id` field and is constrained by ADR-0008, not this one, so ADR-0008
is `chain_id`'s owner; see Deferred Decisions.

The exact canonical binary encoding is defined by ADR-0004.

The exact hash function used by derivation schemes is defined by ADR-0005.

## Normative Rules

### Binary Payload Is Normative

The canonical binary `AddressPayload` is the consensus object.

Textual address strings are not used for state hashing, signature payloads,
state tree keys, or consensus equality.

### Versioned Address Format

Every address includes `address_version`.

Nodes must not infer address version from string length, prefix length, payload
length, checksum behavior, or account type.

### Namespace Separation

Every address includes `address_namespace`.

**Decided.** `address_namespace` is `uint8`, mirroring `domain_id` in
ADR-0007: a small closed registry gets an explicit numeric constant per
value, not a string or a Rust-level enum, so consensus meaning does not
depend on the implementation language's type system.

Namespace registry for `address_version = 1`:

```text
0x00  reserved, invalid for committed addresses
0x01  account
0x02  contract
0x03  validator
0x04  protocol
0x05  bridge
0x06  identity
```

This registry is closed for `address_version = 1`, the same way ADR-0007
closes its SectionId registry "for tree profile `0x0001`". Adding a
namespace is not possible within `address_version = 1`; it requires a new
`address_version`, which is already a major protocol change under Versioned
Address Format above. The general "adding a new address namespace" bullet
in Compatibility below is therefore satisfied by, and does not need a
process separate from, the existing rules for introducing a new
`address_version`.

Namespaces prevent accidental reuse of the same bytes for different protocol
domains.

**Decided: no `asset` namespace.** Native/protocol-level and bridged asset
*definitions* have an ADR-0007 state domain (`assets`, `0x0005`) but no
entry here: they are protocol-curated via a numeric `asset_id` registry,
not permissionlessly derived the way `account`/`contract`/`validator`
addresses are, so they do not need an `AddressPayload` at all. See
ADR-0007's State Domains, "Decided: `assets` is not an `address_namespace`,"
for the full reasoning, including why contract-defined assets and
per-account balances are not this domain either.

### Network Separation

Every address binds to a `network_id`.

An address valid on one HNChain network must not be silently valid on another
network unless cross-network behavior is explicitly specified.

Human-readable prefixes are useful, but they are not sufficient network
separation for consensus. The network identifier must be inside the canonical
payload.

**Decided: `network_id` width and value scheme.** `network_id` is `uint16`.
Values are split into a small registered range and an open, self-assigned
range for devnets:

```text
0x0000           reserved, invalid
0x0001           mainnet
0x0002           testnet
0x0003..0x7FFF   reserved for future centrally registered networks
0x8000..0xFFFF   devnet range: self-assigned per devnet instance, not
                 centrally registered
```

`uint8` (mirroring `domain_id`/`address_namespace`/`SectionId`) was
considered and rejected here specifically: those registries close over a
small, genuinely fixed set of values, but devnets are expected to run many
disposable, concurrent instances (feature branches, CI-ephemeral networks),
and a single shared devnet value would mean two independently spun-up
devnets cannot be distinguished at the address level at all — a real replay
risk if their key material ever overlaps. `uint16` gives the devnet range
65024 self-assigned values without requiring a hash-sized field or a
central registry entry per devnet.

Registering a new mainnet- or testnet-class network (`0x0003..0x7FFF`)
requires a protocol change record, the same as any other closed-registry
addition in this ADR. Picking a `network_id` inside the devnet range does
not: any value in `0x8000..0xFFFF` is valid for `address_version = 1`
without registration, and devnet genesis configuration is responsible for
choosing one that does not collide with other devnets it might interact
with.

### Derivation Scheme Separation

Every address includes `derivation_scheme`.

Nodes must not infer derivation scheme from `algorithm_id` alone. Multiple
address derivation schemes may exist for the same key algorithm.

### Fixed Consensus Equality

Two addresses are equal only if their canonical binary payloads are byte-for-byte
equal after canonical decoding.

Case folding, Unicode normalization, whitespace trimming, or display formatting
must not affect consensus equality. Neither must the checksum algorithm used to
encode an address to text: `AddressPayload` does not carry a checksum field
(Decision, "no `checksum_profile` in `AddressPayload`"), so it cannot
participate in consensus equality by construction, not by convention.

### No Raw Public Keys As Addresses

Raw public keys are not addresses.

An account may rotate or add keys without changing its address if account rules
support stable identity. Conversely, a new address may be derived from a key
descriptor if the chosen derivation scheme defines that behavior.

### Human-Readable Encoding

HNChain should use a Bech32m-style human-readable encoding for user-facing
addresses unless rejected by later implementation analysis.

Reasons:

- lower risk of ambiguous characters than mixed-case encodings
- built-in checksum suitable for copy and paste workflows
- human-readable prefix support
- existing implementation experience in blockchain systems

The checksum protects address entry and transport. It is not a cryptographic
integrity mechanism and must not replace canonical hashing or signature
verification.

**Decided: checksum scheme.** With `checksum_profile` removed from
`AddressPayload` (Decision above), "what does the checksum protect" is
answered by Bech32m's own construction rather than needing a separate
choice: Bech32m computes its checksum over the full typed string — HRP plus
the entire encoded data part — so it covers every payload field and the HRP
together, not `address_body` alone and not a subset chosen per address.

Because the checksum is entirely outside `AddressPayload`, changing the
checksum algorithm later is not a change to any already-issued address's
consensus identity at all, and does not require a new `address_version`:
the same `AddressPayload` bytes can be re-rendered under a different
text-encoding convention without altering what they identify. It is a
change to this specification's (or its successor's) text-encoding
convention, scoped the same way HRP string choices are — not a Structure
Version in ADR-0022's sense, since there is no field whose value changes.

This closes the "what does the checksum protect" half of the original
checksum-scheme open item; "which specific algorithm" stays the existing
Bech32m recommendation above, still hedged ("unless rejected by later
implementation analysis") pending actual implementation experience, not a
remaining architectural fork.

**Decided: HRP scope.** The human-readable prefix (HRP) encodes `network_id`
only. It must not encode `address_namespace` or any other payload field.

This follows the same principle Network Separation above already applies to
`network_id` itself: "Human-readable prefixes are useful, but they are not
sufficient... for consensus. The network identifier must be inside the
canonical payload." Payload is normative; HRP is not. Encoding namespace in
the HRP as well (for example, a distinct prefix per namespace, as some
Cosmos-family chains do) would create a second, non-consensus signal for a
fact the payload already states, with no mechanism forcing the two to agree
— exactly the class of risk already rejected for network separation, just
applied to a different field. A decoded `address_namespace` that disagrees
with what a namespace-specific HRP implied would be an unresolvable
conflict with no principled way to prefer one signal over the other.

Wallets and explorers display `address_namespace` from the decoded payload,
not from the prefix. `address_namespace` does not need its own HRP to be
human-visible.

**Decided: HRP-network_id consistency.** Wallets, CLI tools, and explorers
must decode `network_id` from the payload and verify it matches the network
the presented HRP claims, before displaying or accepting the address for a
transaction targeting that network. A mismatch must be rejected or
prominently surfaced as a warning, not silently accepted.

"Matches" is exact-value equality for the mainnet and testnet HRPs (their
HRP claims exactly `network_id = 0x0001` or `0x0002` respectively) and
range membership for the devnet HRP (its HRP claims `network_id` falls
inside `0x8000..0xFFFF`, per Network Separation's devnet range — not one
fixed value, since devnets are deliberately many and self-assigned). A
devnet-HRP address whose decoded `network_id` falls outside that range is
exactly as much a mismatch as a mainnet-HRP address decoding to
`network_id = 0x0002`.

Nothing prevents constructing a syntactically valid address whose HRP and
encoded `network_id` disagree, since the HRP is not part of consensus and
carries no cryptographic binding to the payload on its own — only the
Bech32m checksum ties the HRP to the byte string as typed, which protects
against transcription errors, not against a deliberately mismatched pair
assembled by an attacker. Checking this consistency is therefore an
application-level responsibility, not something the encoding format
enforces by construction. See Security Considerations, "HRP spoofing."

## Initial Address Namespaces

### Account Address

Used for user accounts and account-controlled state.

**Decided: account address derivation function.**

```text
address_body = HASH_PROFILE_0x0001(
  domain = "hnchain.address.account.v1",
  payload = HNCS(AccountAddressInputV1)
)

AccountAddressInputV1
  u16   address_version = 1
  u16   network_id
  u8    address_namespace = 0x01
  u8    derivation_scheme
  u16   algorithm_id
  bytes public_key
```

`derivation_scheme = 0x01` (`DirectPublicKey`, ADR-0002's `algorithm_id`
identifies which signing algorithm; `derivation_scheme` is a separate,
explicit field per "Derivation Scheme Separation" above, not inferred from
it) for this scheme: `address_body` is a direct hash commitment to the
signing public key, with no additional identity-commitment layer.

`public_key` is the raw signing public key bytes for `algorithm_id`
(ADR-0002) — not a commitment to the full `KeyDescriptor`. `key_role` does
not enter this derivation: the `account` namespace already fixes the
context to account-signing-class keys, and ADR-0002 already discourages
reusing one key across roles, so binding `key_role` into the address would
duplicate a guarantee that belongs to key management, not address
identity.

`algorithm_id` is a field, and `public_key` is variable-length
(HNCS-bounded, not a fixed 32-byte array), because Ed25519's 32-byte key
is not representative of every `algorithm_id` ADR-0002 reserves — a
fixed-width field would break the moment a different-length key algorithm
(for example a reserved post-quantum one) activates.

This resolves "account address derivation function" for the `account`
namespace only. Contract, validator, protocol, and identity address
derivation remain open (see Open Decisions); bridge address derivation is
already resolved separately (Bridge Address below).

### Contract Address

Used for smart contract instances.

Contract address derivation must bind to:

- creator or deployer account
- deployment nonce or unique deployment input
- code commitment
- network identifier
- contract namespace

### Validator Address

Used for validator identity inside consensus and staking.

Validator address must not be treated as equivalent to account address unless a
specific binding is present in validator state.

### Protocol Address

Used for protocol-owned modules such as treasury, governance system contracts,
staking, slashing, bridge registries, and future native modules.

**Decided (genesis list only).** The set of protocol modules reserved at
`address_version = 1` genesis — treasury, governance, staking, slashing,
bridge registry — is fixed once and enumerated in the genesis specification,
not in this ADR. This is a different question from namespace numbering
above: it is about which specific protocol-address values exist within the
already-numbered `protocol` namespace (`0x04`), not about the namespace
registry itself.

**Decided: state domain per module.** The `protocol` namespace names which
addresses exist; it does not say where their state lives. Each genesis
module lands in the ADR-0007 state domain that matches its actual
cardinality, not a single catch-all:

```text
governance  -> governance domain (0x0007)  — one singleton object
treasury    -> system domain (0x0009)      — one singleton object
staking     -> validators domain (0x0006)  — per-validator/delegator records
slashing    -> validators domain (0x0006)  — per-validator penalty history
bridge registry -> bridge domain (0x000A)  — one singleton object
```

The criterion is singleton versus partitioned collection, not topical
similarity: `staking` and `slashing` are not grouped with `treasury` in
`system` just because all three are "protocol modules" — they hold one
record per validator or delegator, the same partitioning shape
`validators` already has, and `system` has no such per-entity structure.
Putting them there would misrepresent a collection as a single
configuration record. `bridge registry` (which external chains and assets
are supported, custody rules) belongs in the `bridge` domain reserved for
bridge objects generally, not `system`, for the same reason ADR-0007
reserved that domain ID in the first place: it is a bridge-domain object,
not a generic one.

`metadata` (`0x0008`) holds none of these: it is protocol-internal
bookkeeping, not reached through any `protocol`-namespace address. Nothing
in the genesis module list is addressed there.

Whether additional protocol modules can be reserved after genesis without a
hard fork, and through what governance process, is deferred — see Deferred
Decisions. It depends on a future governance ADR that does not exist yet,
so this ADR fixes only the genesis-time list and does not attempt to
specify an extension process ahead of that governance ADR.

### Bridge Address

Used for bridge-related accounting and external chain commitments.

Bridge addresses must bind to explicit bridge namespace and chain identifiers to
avoid cross-chain replay and asset confusion.

The bridge address body is a hash commitment over
`(external_chain_id, external_account_reference)` under `HASH_PROFILE_0x0001`,
not the raw external chain identifier or account reference embedded directly.
This keeps the bridge namespace's body the same fixed length as every other
namespace (see Recommended Initial Profile) regardless of how long an
external chain's own identifiers or account references happen to be.

### Identity Address

Used for HN Identity records if identity becomes a distinct protocol namespace.

Identity address semantics must not be overloaded onto account addresses without
an explicit binding model.

## Recommended Initial Profile

This ADR proposes, but does not yet accept as a whole, the following initial
profile. Individual elements are resolved incrementally as they are decided;
each says so explicitly. Elements without such a note are still proposed,
not accepted.

- canonical binary payload for consensus
- Bech32m-style text encoding for wallets, CLI, RPC, and explorer
- lowercase HRP
- separate HRPs for mainnet, testnet, and local development networks — one
  devnet HRP covering the whole self-assigned devnet `network_id` range, not
  one HRP per devnet instance; see HRP-network_id consistency below for
  exact-match versus range-match semantics
- network identifier inside the binary payload
- **Decided:** 32-byte address body, uniform across every namespace
  (account, contract, validator, protocol, bridge, identity) for
  `address_version = 1`. See "Address Body Length" below.
- variable-length payload support for future address versions

### Address Body Length

**Decided.** `address_body` is 32 bytes for every namespace under
`address_version = 1`. This is not a per-namespace choice: every namespace's
derivation direction above is a `HASH_PROFILE_0x0001` consumer (account and
contract directly; validator and identity by the same pattern; protocol by
reservation over the same 32-byte space; bridge via an explicit hash
commitment, see Bridge Address above), and ADR-0005 fixes that profile's
digest length at exactly 32 bytes for every active consensus hash domain.
A uniform body length follows from that rather than being an independent
decision to defend namespace by namespace.

This is a profile choice for `address_version = 1`, not a universal
invariant: `address_namespace` and `address_version` already exist as
separate fields in `AddressPayload`, so a future `address_version` can
introduce a different (including variable, or namespace-dependent) body
length — for post-quantum key material, for example — without
reinterpreting `address_version = 1` addresses or requiring a namespace
split that `address_version = 1` does not have.

32 bytes is also the value `hn-state::key::OBJECT_ID_MAX_LEN` (64 bytes)
was chosen ahead of this decision to accommodate; no change to that
constant is needed (see its `TODO(ADR-0003)` note).

## Rejected Options

### Address Equals HASH(PublicKey)

Advantages:

- simple
- compact
- common in blockchain systems

Disadvantages:

- couples address identity to one key representation
- complicates key rotation
- hides algorithm choice
- makes post-quantum migration harder
- does not naturally support contract, protocol, bridge, and identity namespaces

Rejected as the general HNChain address model.

It may still appear as one derivation scheme inside the versioned address
framework.

### Raw Public Key Address

Advantages:

- avoids hash collision discussion for key-derived addresses
- direct verification of address-to-key relation

Disadvantages:

- large addresses
- exposes key material before first use
- poor fit for post-quantum public key sizes
- weak UX
- difficult namespace separation

Rejected.

### Hex String Address

Advantages:

- simple tooling
- familiar to EVM users

Disadvantages:

- weak error detection unless an additional checksum scheme is added
- less ergonomic for manual transfer
- case-checksum schemes introduce case-handling pitfalls

Rejected for the default user-facing format.

## Security Considerations

Cross-network replay:

- Risk: an address from one network is accepted on another network.
- Mitigation: include `network_id` in canonical payload and signing context.

HRP spoofing:

- Risk: an address is presented with a mainnet-looking HRP while its decoded
  `network_id` actually names a different network (or the reverse), because
  the HRP is not part of consensus and carries no binding to the payload's
  content beyond the Bech32m checksum protecting against transcription
  errors, not against a deliberately assembled mismatched pair.
- Mitigation: wallets, CLI tools, and explorers must decode `network_id` and
  verify it matches the presented HRP before displaying or accepting the
  address, rejecting or prominently warning on mismatch (Human-Readable
  Encoding above). This is the same class of risk as Address truncation
  below: a human-facing display convention lying about what the canonical
  payload actually contains.

Namespace confusion:

- Risk: account, contract, validator, protocol, bridge, or identity addresses are
  interpreted interchangeably.
- Mitigation: mandatory `address_namespace`.

Algorithm migration failure:

- Risk: addresses cannot survive key algorithm migration.
- Mitigation: avoid raw-key addresses and require `derivation_scheme`.

Checksum misuse:

- Risk: UI checksum is treated as cryptographic integrity.
- Mitigation: checksum is only an input error detector; consensus uses canonical
  bytes, hashes, and signatures.

Unicode and display attacks:

- Risk: visually similar characters or normalization change displayed address
  meaning.
- Mitigation: restrict text encoding alphabet and reject non-canonical display
  forms.

Address truncation:

- Risk: UI displays shortened addresses and users approve the wrong target.
- Mitigation: wallets and explorers must use documented display rules and show
  enough information for high-value operations.

## Compatibility

Adding a new address namespace or derivation scheme can be backward-compatible
only if:

- it has a unique identifier
- canonical encoding is specified
- unsupported nodes have deterministic rejection behavior
- state transition rules for the namespace are defined

For `address_namespace` specifically: since its registry is closed for
`address_version = 1` (Namespace Separation above), "adding a new address
namespace" can only happen by introducing a new `address_version`. The four
conditions above apply to that new version's namespace registry as a whole;
no namespace-specific process beyond them is needed.

Changing equality, decoding, checksum validation, or derivation semantics for an
existing address version is a major protocol change.

## Related Specifications

- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0022-protocol-versioning.md`
- `docs/specs/core/address-format.md`
- `docs/specs/core/canonical-serialization.md`
- `docs/specs/core/transaction-format.md`
- `docs/specs/core/genesis.md`

## Open Decisions

- final mainnet, testnet, and devnet human-readable prefixes
- address body derivation function for `contract`, `validator`, `protocol`,
  and `identity` namespaces (`account` is resolved: Account Address above;
  `bridge` is resolved separately: Bridge Address)
- contract address derivation inputs
- bridge chain identifier format
- display and truncation requirements for wallets and explorers

Resolved and removed from this list: "whether addresses bind directly to
key descriptors or identity commitments" — for the `account` namespace,
directly to the raw public key, not a `KeyDescriptor` commitment (Account
Address above). Other namespaces may answer this differently when their
own derivation inputs are decided.

## Deferred Decisions

- protocol namespace extension process: whether protocol modules beyond the
  `address_version = 1` genesis list (treasury, governance, staking,
  slashing, bridge registry) can be reserved later without a hard fork, and
  through what governance process. Depends on a future governance ADR that
  does not exist yet; not a blocker for accepting this ADR's genesis-time
  list.
- `chain_id` format, width, and value scheme: out of scope for this ADR
  (see Decision, "no `chain_id` in `AddressPayload`"). Owned by ADR-0008
  (Block Format), the ADR that constrains `docs/specs/core/genesis.md`,
  where `chain_id` first appears with concrete (if still conceptual)
  structure. Not a blocker for this ADR, since `AddressPayload` does not
  carry `chain_id`.

## References

- BIP 173: Base32 address format for native v0-16 witness outputs
  https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki
- BIP 350: Bech32m format
  https://github.com/bitcoin/bips/blob/master/bip-0350.mediawiki
- CAIP-2: Blockchain ID Specification
  https://standards.chainagnostic.org/CAIPs/caip-2
