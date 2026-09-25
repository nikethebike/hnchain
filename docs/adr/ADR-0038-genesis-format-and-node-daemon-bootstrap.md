# ADR-0038: Genesis Format And Node Daemon Bootstrap

Status: Accepted

Date: 2026-09-25

Version: 0.1.0

Depends On:

- ADR-0002: Cryptographic Identity
- ADR-0003: Address Format
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms
- ADR-0007: State Tree
- ADR-0010: Validator Set Model
- ADR-0023: Tokenomics And Economic Model
- ADR-0024: HNCOIN Monetary Policy
- ADR-0036: Basic P2P Networking
- ADR-0037: Multi-Node Consensus Wiring

Supersedes: None

Referenced By:

- ADR-0039: Devnet Restart Recovery (adds `hn-node`'s missing
  connection-drop reconnection and passive height-observation
  catch-up, closing this ADR's own restart-recovery gap; also adds
  explicit stdout flushing to `hn-node`'s log lines, since this ADR's
  own "no custom signal handler" decision means a killed process's
  last unflushed line could otherwise be lost)

## Context

`docs/specs/core/genesis.md` (Draft since 2026-07-18) sketches a
conceptual `GenesisHeader`/`GenesisManifest` but explicitly defers every
concrete detail to its own "Open Architecture Decisions": final genesis
manifest fields, final chain ID format, final document commitment
procedure, final genesis state format, final genesis hash profile, and
final initial validator set commitment. `docs/adr/ADR-0024-hncoin-
monetary-policy.md` already fixes the genesis allocation *amounts*
(`1,000,000,000 HNCOIN` total, split `800M`/`100M`/`100M` across the
Reserve/Founder/Community accounts) and defers *who holds the keys* to
`docs/specs/core/genesis-security.md` — itself Draft, with every
custody threshold, custodian selection policy, and the Founder vesting
schedule still explicitly Open.

`hn-node` (ADR-0037) is a real multi-process validator but boots from
hard-coded devnet arithmetic (`validator_id = [index; 32]`, a fixed
`[0x11; 32]` validator-set commitment, peer addresses derived from one
shared `--base-port`) — explicitly named there as scaffolding, not a
real config or genesis mechanism.

The user's own framing: assemble `hn-node` into an actual daemon (real
config, real genesis loading, real DB init, real start/stop), and
finally give `genesis.md` a concrete format now that its economically
load-bearing pieces (HNCOIN allocation, validator minimum bond, epoch
length) are already decided elsewhere.

**What this ADR does not attempt**, stated up front because it bounds
everything below: it does not pick real genesis validators (who
actually runs HNChain's first validators is an organizational decision
this project has not made), it does not pick real custody keys or
thresholds for the three HNCOIN allocation accounts (`genesis-
security.md`'s own Open Decisions, untouched here), and it does not
wire genesis into a real `BlockHeader` (ADR-0008 itself lists "genesis
block compatibility rules" as blocked on this document, and no concrete
`BlockHeader` type exists anywhere in this codebase yet — the same gap
every consensus/networking pass this session has already named). This
ADR defines the genesis *format* and *loading mechanism*, and ships one
devnet example genesis file, clearly labeled, to actually run and test
the daemon against.

## Decision

### Decided: Genesis File Format — JSON, Canonicalized Through HNCS For Hashing

The human-authored/audited genesis source file is JSON. `serde_json` is
already an accepted dependency in this workspace (`hn-hncs`/`hn-state`
already depend on it, today only as a dev-dependency for conformance
test vectors) — promoted here to a real dependency of `hn-node`, used
only as `serde_json::Value` walked and extracted manually (no `#[derive
(Deserialize)]`, no new `serde_derive` proc-macro usage anywhere) —
matching the explicit, hand-written-parsing posture this project's CLI
flag parser (ADR-0037) already established, extended to a nested file
format where hand-rolling a parser from scratch would be needlessly
error-prone. A dedicated `toml` dependency was considered and rejected
purely because nothing in the lockfile already pulls it in, unlike
`serde_json`.

JSON is the *source*; it is never the *hashed* representation.
`GenesisManifest::encode()` re-encodes the parsed structure as
canonical HNCS bytes (the same convention every other hashed protocol
object in this codebase already uses), so `genesis_hash` never depends
on the source file's whitespace, key order, or number formatting —
sidestepping "canonical JSON" as a concept entirely, which the wider
ecosystem has never converged on a single answer for.

All `u128` monetary amounts in the JSON source are encoded as decimal
**strings**, not JSON numbers — JSON numbers are only safely
interoperable up to `2^53`, and genesis allocation amounts
(`800,000,000 * 10^9 hnit ≈ 8×10^17`) exceed that, even though they fit
comfortably in `u128` (and even `u64`) once parsed. This matches
established genesis-file convention elsewhere in the wider blockchain
ecosystem (e.g. Cosmos SDK's own `genesis.json` coin amounts), not
invented fresh here.

### Decided: `GenesisManifest` Schema

```text
GenesisManifest
  manifest_version : u16
  chain_id         : u8
  network_id       : u16
  genesis_time     : u64
  genesis_message  : string, <= GENESIS_MESSAGE_MAX_LEN (512 bytes)
  validators       : list<GenesisValidator>, <= MAX_GENESIS_VALIDATORS (1024)
  allocations      : GenesisAllocations (fixed, not a list -- see below)

GenesisValidator
  validator_id              : 32 bytes
  consensus_key_algorithm_id : u16
  consensus_key_public_key   : bytes, <= PUBLIC_KEY_MAX_LEN
  bonded_stake               : u128

GenesisAllocations
  reserve   : GenesisAccount
  founder   : GenesisAccount
  community : GenesisAccount

GenesisAccount
  address : 32 bytes
  amount  : u128
```

A deliberate merge of genesis.md's own conceptual `GenesisHeader` and
`GenesisManifest` into one concrete type: with no real `BlockHeader` to
split the "header" role out into yet (see "What this ADR does not
attempt," above), keeping two structures whose only real difference was
which one nominally "commits to" the other added no value this pass.
`initial_state_root` is a *computed*, not authored, field — see
"Decided: Initial State Root," below — so it does not appear in the
authored JSON source at all, only in the derived, hashed
`GenesisManifest` value the loader builds in memory.

`allocations` is a fixed three-field structure, not a generic list:
ADR-0024's own "Genesis Allocation Completeness" rule requires *all
three* named accounts to be explicitly defined, each for its own exact
decided amount — a generic list could accidentally omit one, duplicate
one, or accept an arbitrary split that merely sums correctly. The
loader validates each of the three amounts against its own fixed
constant individually (see "Decided: New `hn-state` Constants," below),
not just that the total equals `GENESIS_SUPPLY`.

`GenesisValidator.validator_id` and `GenesisAccount.address` are taken
**as given** from the file, never derived by the loader. Both are
genuinely open elsewhere in this project: `validator_id`'s width is
decided (32 bytes, ADR-0012) but its derivation is not (ADR-0010 never
defines one), and the three allocation accounts' real controlling keys
are `genesis-security.md`'s own still-open custody decision. Inventing
a derivation formula here would be exactly the kind of unverified,
made-up-from-memory value this project has repeatedly avoided
elsewhere (ADR-0011's leader-election formula, ADR-0009's timeout
constants) — the file states the address; the loader trusts it
structurally (valid length, valid Ed25519 point where a public key is
involved) and no further.

`GenesisValidator` carries only a `consensus_key` — no network/handshake
key. This is not an omission: ADR-0036 already decided P2P peer
identity is deliberately *not* a consensus-visible, network-bound
object (`peer_id` lives entirely outside `AddressPayload`/genesis).
Each node's own network keypair is local operator configuration (see
"Decided: Node Config," below), never published in genesis.

### Decided: Genesis Hash

```text
genesis_hash = HASH_PROFILE_0x0001("hnchain.genesis.v1", HNCS(GenesisManifest))
```

New ADR-0005 domain tag, added alongside this decision per the
established registration discipline. `GenesisManifest::encode()` only
— no `decode()` is provided. Nothing in this codebase reconstructs a
`GenesisManifest` from raw HNCS bytes; JSON parsing is the only
production path that produces one, and building an unused decode path
"for consistency" would be exactly the kind of speculative surface this
project avoids elsewhere ("don't design for hypothetical future
requirements").

### Decided: Initial State Root

Genesis's write-set is exactly the entries the loader itself produces:
one `ValidatorRecordV1` leaf per `GenesisValidator` (`Active`,
`bonded_stake == voting_power`, no `pending_unbonding`) and one
`BalanceValueV1` leaf per `GenesisAllocations` entry — the same
minimal, envelope-free account-seeding shape
`single_validator_chain_integration.rs` already established for
pre-funding a test account (a genesis-funded account has no `Identity`/
`Envelope` leaf until its first real, bootstrap-signed transaction; this
is not new behavior, just the first *production* use of a pattern this
session's own tests already relied on). `initial_state_root` is
computed by feeding that write-set through the existing, unmodified
state-tree machinery: `hn_state::leaf_for_write` per entry,
`hn_state::compute_state_root` against a freshly built
`hn_state::EmptyHashTable` — exactly the mechanism a real block-
processing pipeline would use, not a parallel one invented for genesis
alone.

### Decided: New `hn-state` Constants

`hn-state` gains a new `hncoin` module — the same crate
`MINIMUM_VALIDATOR_BOND`/`UNBONDING_PERIOD_BLOCKS` already live in, for
the same reason (a protocol constant ADR-0023/ADR-0024 already decided,
with no code consumer until now):

```text
HNCOIN_DECIMALS          : u32 = 9
MAX_SUPPLY                : u128 = 1_000_000_000 * 10^9
GENESIS_SUPPLY             : u128 = MAX_SUPPLY
RESERVE_ALLOCATION         : u128 =   800_000_000 * 10^9
FOUNDER_ALLOCATION         : u128 =   100_000_000 * 10^9
COMMUNITY_ALLOCATION       : u128 =   100_000_000 * 10^9
```

Values copied directly from ADR-0024's own table, not re-derived —
this ADR is not a second owner of them (ADR-0024's own "One Owning
Document" rule).

### Decided: Genesis Validation Rules

The loader rejects a genesis file that:

- has an unsupported `manifest_version`
- declares a `chain_id` of `0` (`hn_core::ChainId`'s own reserved
  value)
- has zero validators, more than `MAX_GENESIS_VALIDATORS`, or a
  duplicate `validator_id`/`consensus_key_public_key` across entries
- has any validator with `bonded_stake < MINIMUM_VALIDATOR_BOND`
  (ADR-0023) or an invalid Ed25519 public key
- has any allocation account amount not *exactly* equal to its own
  fixed constant, or a duplicate address across the three allocation
  accounts and the validator set
- has a `genesis_message` exceeding `GENESIS_MESSAGE_MAX_LEN`

Every check is structural/arithmetic, computable offline with no
network or prior chain state — matching genesis.md's own "Security
Requirements": "Genesis data must have canonical serialization,"
"Genesis construction must reject any configuration where
`allocation_sum != 1,000,000,000`" (ADR-0024's own wording, now a real
enforced check for the first time).

### Decided: DB Init — Genesis Marker, Not A New Storage Table

On open, `hn-node` reads a fixed, out-of-band sentinel state key —
`hash_profile_0x0001("hnchain.node.genesismarker.v1", &[])`, deliberately
*outside* ADR-0007's own domain registry, since this is node-local
integrity bookkeeping, not consensus state anyone else ever reads or
commits to a real state root. If absent (fresh database), the loader
validates the configured genesis file, applies its write-set plus the
marker (`genesis_hash` bytes) in one commit, and proceeds. If present,
the loader recomputes the configured genesis file's own `genesis_hash`
and compares — a mismatch is a hard startup error (`--data-dir` and
`--genesis` disagree about which chain this is), matching genesis.md's
own §8: "Nodes must reject genesis data that does not match the
configured chain ID and genesis hash." A dedicated second `redb` table
was considered and rejected: it would need a new `hn-storage` method
just for this one node-local value, where reusing the existing
`StateReader`/`StateWriter` path through one well-documented sentinel
key needs no storage-layer change at all.

### Decided: Node Config — Extend The Existing Flag Parser, No New Format

`hn-node`'s config gains new flags rather than a second, independent
file format: `--genesis <path>` (required), `--listen <addr>`
(replaces `--base-port` arithmetic), `--peer <addr>` (repeatable,
replaces the `validator_count`-derived peer list), `--consensus-key-
seed <hex32>` / `--network-key-seed <hex32>` (this operator's own
private key material — devnet-simple, a raw seed, not a keystore file;
real key-management hardening is explicitly future work, the same
scope boundary `genesis-security.md` already draws around its own
ceremony details), and `--base-timeout-ms` (unchanged). `--validator-
index`/`--validator-count`/`--base-port` are removed outright, not
deprecated-but-kept: they described devnet-only arithmetic identity
that a real genesis-driven validator set makes actively wrong to keep
(this project's own "avoid backwards-compatibility hacks" discipline —
"if you are certain something is unused, delete it completely").

`--config <path>` reads a file of the same `--flag value` tokens (one
or more per line, blank lines and `#`-prefixed lines ignored) and
splices them into the argument list before parsing — the *same* parser
consumes both a config file and literal CLI flags, so there is no
second format to keep in sync and no new dependency, continuing this
session's now-established minimal-dependency posture for configuration
surfaces (no `clap`, and now no `toml` either).

### Decided: Own Validator Identity Resolution

The node's own `validator_id` is resolved once at startup by matching
its configured `--consensus-key-seed`-derived public key against
`GenesisValidator.consensus_key_public_key` across the loaded genesis
validator list. No match is a hard startup error: this pass does not
support a non-validating "full node" mode (a node that syncs and
gossips but never votes) — a real, separate future design, not
attempted here since nothing today needs it and no shape for it has
been discussed.

### Decided: Connection-Layer Simplification — Drop Validator-Index Peer Resolution

ADR-0037's `resolve_validator_index` (matching a peer's handshake
`node_key` against a formulaic per-index network key) is removed, not
adapted: with real, operator-supplied network keys there is no
formula left to check against, and — recognized while working through
this pass, not assumed going in — it was never actually *necessary*
for correctness even in ADR-0037's own design. Consensus messages
authenticate themselves (`ConsensusVote`/`QuorumCertificate` carry
their own `validator_id` and signature, checked against the genesis-
loaded `consensus_key` — completely independent of which TCP
connection or network identity delivered the bytes); the connection
layer's only real job is admission control (did this peer complete a
valid `Hello` handshake at all, proving possession of *some* legitimate
network key) and routing (broadcast to every connection whose
handshake succeeded). Tying a connection to a specific validator index
was ADR-0037 devnet bookkeeping that added complexity without adding a
real security property — this pass simplifies `hn-node`'s connection
state down to "which connections are handshake-established," dropping
the `conn_by_validator`/`resolve_validator_index` machinery entirely.

### Decided: No Custom Signal Handling

"Stop" means OS-level process termination (Ctrl+C, `SIGTERM`,
`taskkill`) — `hn-node` installs no custom signal handler. Two reasons,
not one: `hn_storage::RedbStateStore` already commits one write-set per
`redb` transaction with the ACID guarantee that an interrupted
transaction leaves the database completely unchanged (ADR-0033), so an
ungraceful kill at any point already leaves no corrupted state to clean
up; and every safe, `std`-only path to catching `SIGINT`/`Ctrl+C`
requires either `unsafe` FFI (forbidden outright,
`#![forbid(unsafe_code)]`, workspace-wide) or a new dependency
(`ctrlc` or equivalent) purely for a marginally cleaner shutdown log
line, no different in actual safety terms from just letting the OS
terminate the process. Given nothing else needs cleaning up (sockets
and threads are reclaimed by the OS on exit regardless), the dependency
was not worth adding this pass — see "Rejected Options," below.

### Decided: Devnet Example Genesis

`hn-node/genesis/devnet.json` — four validators and the three
allocation accounts, every key deterministically seeded (same
`Ed25519KeyPair::from_seed` convention this whole session has already
used for devnet identity), `chain_id = hn_core::ChainId::HNCHAIN.get()`
(the one real, already-assigned HNChain lineage value — not a devnet
placeholder; ADR-0037's own bare `chain_id: 1` literal was already this
value, just not spelled via the named constant), `genesis_message`
explicitly labeled "not for production use." This file is committed
scaffolding for running and testing the daemon, exactly parallel to how
ADR-0037's own multi-process integration test used deterministic devnet
keys — never to be confused with, or reused as, HNChain's real genesis
once one is actually produced through the real (still entirely
undecided) key-generation ceremony `genesis-security.md` describes.

## Explicitly Not Resolved

- real genesis validator identities (an organizational decision, not a
  technical one)
- real custody keys/thresholds for the three allocation accounts
  (`genesis-security.md`'s own Open Decisions, unchanged)
- the document-commitment procedure (`whitepaper_hash`/
  `specification_hash` and friends) — genesis.md's own §6 already lists
  every open sub-question (included files, ordering, normalization,
  archive format); this ADR does not invent an unreviewed procedure
  just to fill in a byte-exact hash. `GenesisManifest` has no document-
  commitment fields at all this pass, not placeholder-`None` ones —
  adding them is real future scope, not a gap silently left in the
  schema.
- wiring genesis into a real `BlockHeader`/block 0 — `hn_state::
  BlockHeader` now exists (ADR-0008), so this item's own blocker has
  changed: not "no type exists," but "nothing yet constructs a real
  genesis block 0 from `GenesisManifest`," a distinct, still-unstarted
  integration task
- non-validating "full node" mode
- connection retry after an established link drops (ADR-0037's own
  already-recorded gap, unchanged)
- any encrypted/hardware-backed key storage for `--consensus-key-seed`/
  `--network-key-seed` (raw seed flags only, this pass)

## Rejected Options

### A Dedicated `toml`/Second Config Format

Rejected: `serde_json` is already in this workspace's lockfile;
`toml` is not. Reusing the existing CLI flag parser for a file of the
same tokens needs no new dependency and no second format to keep
documented and in sync with the flag set.

### A Signal-Handling Dependency (`ctrlc` Or Equivalent)

Rejected for this pass: the only thing a handler would add over letting
the OS terminate the process is a cleaner shutdown log line — `redb`'s
existing transactional guarantee already means an ungraceful kill
leaves no corrupted state, so there is no *safety* property being
traded away, only cosmetics. Revisit if a future pass adds real
in-memory state that genuinely needs an explicit flush before exit.

### Deriving `validator_id` From `consensus_key`

Rejected: no derivation rule is decided anywhere in this project
(ADR-0012 fixes only the width). Inventing one now, unreviewed, to make
the genesis schema "cleaner" would be exactly the class of unverified,
remembered-not-checked value this project has repeatedly avoided
(ADR-0011's proposer-priority formula, ADR-0009's timeout constants).
The file states `validator_id` explicitly instead.

### Keeping ADR-0037's Index-Derived Peer/Identity Config Behind A Flag

Rejected: maintaining two parallel identity/topology mechanisms (real
genesis-driven vs. devnet index arithmetic) is exactly the kind of
backwards-compatibility hack this project's own conventions warn
against once the arithmetic version is not just superseded but
actively *wrong* against a real genesis validator set. The devnet
example genesis (above) already provides an equally low-friction way to
run a local multi-node devnet.

## Security Considerations

Genesis mismatch:

- Risk: two nodes configured with different genesis files (or a
  tampered one) silently form separate, incompatible networks, or one
  node accepts a modified allocation/validator set.
- Mitigation: the genesis-marker comparison on every DB open ("Decided:
  DB Init"), and `genesis_hash`'s own canonical HNCS re-encoding making
  the comparison independent of the source file's exact bytes/
  formatting.

Raw seed key material in config:

- Risk: `--consensus-key-seed`/`--network-key-seed` (or a `--config`
  file containing them) is a plaintext secret on disk/in process
  arguments.
- Mitigation: none added this pass beyond documenting the gap plainly —
  named explicitly in "Explicitly Not Resolved," not glossed over; real
  operators of a real network need a real key-management story this
  ADR does not attempt to provide.

Genesis allocation validation bypass:

- Risk: a malformed or malicious genesis file understates/overstates
  an allocation, or omits one of the three required accounts.
- Mitigation: "Decided: Genesis Validation Rules" checks each of the
  three amounts against its own fixed constant individually, not merely
  that the total sums correctly — closing exactly the gap a sum-only
  check would leave (e.g. Founder overstated, Reserve understated by
  the same amount, sum still correct).

## Compatibility

This ADR introduces no consensus-visible wire format: `GenesisManifest`
is loaded and hashed locally by each node at startup, never transmitted
over `hn-network`, and does not change any already-decided ADR-0006/
ADR-0012/ADR-0018 wire type. Adding real document-commitment fields to
`GenesisManifest` later is an additive schema change (new optional
fields), not a breaking one, since `manifest_version` already exists to
gate it.

## Open Decisions

- document commitment procedure (genesis.md §6, unchanged)
- real genesis validator selection and real allocation-account custody
  (organizational, not technical)
- non-validating full-node mode
- key-management hardening for config-supplied seeds
- genesis's eventual integration into a real `BlockHeader`/block 0

## Related Specifications

- `docs/specs/core/genesis.md`
- `docs/specs/core/genesis-security.md`
- `docs/adr/ADR-0023-tokenomics-and-economic-model.md`
- `docs/adr/ADR-0024-hncoin-monetary-policy.md`
