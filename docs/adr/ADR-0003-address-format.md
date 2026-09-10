# ADR-0003: Address Format

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0001: Extended Account-Based State Model
- ADR-0002: Cryptographic Identity

Supersedes: None

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
  checksum_profile
```

Conceptual text structure:

```text
hrp + separator + encoded(AddressPayload)
```

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

### Network Separation

Every address binds to a `network_id`.

An address valid on one HNChain network must not be silently valid on another
network unless cross-network behavior is explicitly specified.

Human-readable prefixes are useful, but they are not sufficient network
separation for consensus. The network identifier must be inside the canonical
payload.

### Derivation Scheme Separation

Every address includes `derivation_scheme`.

Nodes must not infer derivation scheme from `algorithm_id` alone. Multiple
address derivation schemes may exist for the same key algorithm.

### Fixed Consensus Equality

Two addresses are equal only if their canonical binary payloads are byte-for-byte
equal after canonical decoding.

Case folding, Unicode normalization, whitespace trimming, or display formatting
must not affect consensus equality.

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

Initial derivation direction:

```text
account_address = ADDRESS_DERIVE(
  network_id,
  namespace = account,
  derivation_scheme,
  key_descriptor_or_identity_commitment
)
```

Exact derivation waits for ADR-0004 and ADR-0005.

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
- separate HRPs for mainnet, testnet, and local development networks
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
- `docs/specs/core/address-format.md`
- `docs/specs/core/canonical-serialization.md`

## Open Decisions

- final mainnet, testnet, and devnet human-readable prefixes
- binary size and type of `network_id`
- address body derivation function
- whether addresses bind directly to key descriptors or identity commitments
- contract address derivation inputs
- bridge chain identifier format
- display and truncation requirements for wallets and explorers

## Deferred Decisions

- protocol namespace extension process: whether protocol modules beyond the
  `address_version = 1` genesis list (treasury, governance, staking,
  slashing, bridge registry) can be reserved later without a hard fork, and
  through what governance process. Depends on a future governance ADR that
  does not exist yet; not a blocker for accepting this ADR's genesis-time
  list.

## References

- BIP 173: Base32 address format for native v0-16 witness outputs
  https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki
- BIP 350: Bech32m format
  https://github.com/bitcoin/bips/blob/master/bip-0350.mediawiki
- CAIP-2: Blockchain ID Specification
  https://standards.chainagnostic.org/CAIPs/caip-2
