# ADR-0036: Basic P2P Networking

Status: Proposed

Date: 2026-09-25

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0009: Consensus Architecture
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0018: P2P Protocol Messages

Supersedes: None

## Context

`hn-network` has been a pure stub. ADR-0018 already defines the
conceptual boundary (a versioned message envelope, logical channels,
message families, capability negotiation, transport independence,
staged cheap rejection) but explicitly leaves the concrete schemas,
registries, and — critically — the transport itself as Open Decisions
("final envelope schema," "final channel registry," "final message
type registry," "handshake schema," "initial transport profiles," ...).
This ADR closes the slice the user scoped explicitly: node discovery,
peer handshake, and message serialization for block/transaction/vote
propagation. It deliberately does not attempt every Open Decision
ADR-0018 lists — compression, encryption, rate limiting, peer scoring,
evidence/checkpoint/snapshot/light-client message families, and (most
consequentially) a real transport all stay out of scope, named
explicitly rather than guessed at.

Three genuine forks were asked rather than decided unilaterally, given
how consequential and hard to reverse each is:

1. **Transport reality**: pure protocol logic only, proven via an
   in-process test double, no real sockets — chosen over a real
   synchronous `std::net` transport or introducing `tokio` (no async
   runtime exists anywhere in this workspace yet, and picking one would
   shape `hn-node`/`hn-rpc`'s own future architecture, not just this
   crate's). Mirrors the exact "pure state machine first, real wiring
   later" split that worked for `hn-consensus` (ADR-0034 skeleton →
   ADR-0035 wiring).
2. **Node identity**: every peer (validator or not) gets its own
   network identity, distinct from `validator_network` — chosen over
   scoping authenticated handshakes to validators only, or skipping
   real authentication entirely.
3. **Discovery ambition**: a static bootstrap peer list plus
   gossip-style `peer_announce`/`peer_request` propagation — chosen
   over a bootstrap-list-only design, and well short of a DHT (ADR-0018
   itself never names one).

## Decision

### Node Identity

**New `KeyRole::NodeIdentity = 0x07`** (`hn-crypto`), distinct from
`KeyRole::ValidatorNetwork = 0x03` — ADR-0018's own RFC text already
draws this line ("node identity does not grant validator authority"),
and `ValidatorNetwork`'s own documentation already scopes it to
"validator peer-identity / network-layer operations" specifically, not
every peer. A non-validator full node, RPC-only node, or light client
now has a real role to hold a network identity key under. ADR-0002's
"Key Roles" conceptual list is synced to include `node_identity`.

**`peer_id(descriptor: &KeyDescriptor) -> Digest`** (`hn-network`, not
`hn-crypto`): `HASH_PROFILE_0x0001("hnchain.network.peerid.v1",
HNCS(PeerIdInputV1))` where `PeerIdInputV1 = { algorithm_id, public_key
}` — a new ADR-0005 domain tag, backfilled the same way
`hnchain.governance.proposal.v1`/`hnchain.evidence.v1` were each added
when their own owning ADR first needed them.
`hnchain.p2p.message.v1` was already reserved (unused until now) for
`message_id` below.

**Deliberately not network-bound**, unlike `account_address_body`
(which folds in `network_id`/`address_namespace`): `peer_id` is a pure
function of the identity key alone. This is not an oversight — a peer
identity is not a consensus-visible object needing replay protection
across networks the way an account address is; that binding already
happens at the handshake/envelope level (`chain_id`/`network_id`
fields, below), and real-world P2P identity schemes (libp2p peer IDs,
devp2p enode IDs) are likewise network-agnostic by design, letting one
identity key connect to multiple networks.

### Message Envelope

**`P2PMessageEnvelopeV1`** (`hn-network::envelope`):

```text
P2PMessageEnvelopeV1
  envelope_version: u16       (= 1)
  protocol_version: ProtocolVersion (hn_core, 3x u16)
  chain_id: ChainId            (hn_core, u8, ADR-0006)
  network_id: u16              (ADR-0003)
  channel: Channel             (new closed registry, below)
  message_type: MessageType    (new closed registry, below)
  message_id: Digest
  payload: bytes (bounded)
```

`message_id = HASH_PROFILE_0x0001("hnchain.p2p.message.v1",
HNCS(channel || message_type || payload))` — content-addressed
(same content always produces the same ID, useful for gossip
deduplication: two peers relaying the same announcement produce
identical `message_id`s), computed over the envelope's own other
fields, not a caller-supplied nonce.

**No generic `capabilities`/`compression_profile`/`encryption_profile`/
`authentication` fields in V1**, despite ADR-0018's conceptual sketch
listing them on every envelope — a deliberate reading, not a shortfall:

- `capabilities` only makes sense negotiated once, at handshake — it
  is part of `Hello`'s own payload (below), not repeated on every
  message.
- `compression_profile`/`encryption_profile` have no decided profile to
  name yet (ADR-0018's own Open Decisions) — V1 simply means
  "uncompressed, unencrypted at this layer" for now; a future
  `envelope_version` adds the fields once a profile exists to put in
  them, matching ADR-0022's own versioning philosophy (don't reserve a
  field for a feature with nothing decided to put in it).
- `authentication` is message-type-specific already, per ADR-0018's own
  "Authentication" rule — `ConsensusVote`/`QuorumCertificate`/
  `TransactionEnvelope` all already carry their own signature envelopes
  inside their payload; `Hello` carries its own (below); nothing else
  in this pass needs a second, redundant envelope-level signature.

### Channel And Message Type Registries

**`Channel`** (`hn-network::registry`), closed for this profile, all
ten of ADR-0018's own named channels get an ID (reserving the ones this
pass does not implement, the same "reserve the registry slot, name the
blocker" pattern `TxType` already used for `contract_deploy`/
`contract_call`/`system`): `Handshake = 0x01`, `PeerDiscovery = 0x02`,
`Transactions = 0x03`, `Blocks = 0x04`, `Consensus = 0x05`,
`Evidence = 0x06` (reserved — blocked on ADR-0015 evidence message
design), `Sync = 0x07` (reserved — blocked on ADR-0016), `Snapshot =
0x08` (reserved — blocked on ADR-0016), `LightClient = 0x09` (reserved
— blocked on ADR-0017), `Control = 0x0A`.

**`MessageType`** (`hn-network::registry`), closed for this profile,
covering the RFC's own named message families: `Hello = 0x01` (merges
the RFC's separate `hello`/`capabilities` into one handshake message —
see "Handshake," below), `PeerAnnounce = 0x02`, `PeerRequest = 0x03`,
`TransactionAnnounce = 0x04`, `TransactionRequest = 0x05`,
`TransactionResponse = 0x06`, `BlockAnnounce = 0x07`, `BlockRequest =
0x08`, `BlockResponse = 0x09` (merges the RFC's separate
`block_header_request`/`block_body_request` into one — no concrete
`BlockHeader` type exists anywhere in this codebase yet, ADR-0030/
ADR-0033/ADR-0035's own recurring named gap, so there is no header to
request separately from the body yet), `ConsensusProposal = 0x0A`,
`ConsensusVote = 0x0B`, `QuorumCertificateMessage = 0x0C`, `Ping =
0x0D`, `Pong = 0x0E`, `Disconnect = 0x0F` (reserved — no payload schema
or handling logic this pass, just the ID, since they need no new
design work whenever implemented), then reserved IDs `0x10`-`0x18` for
the RFC's remaining eight families (`evidence_announce`/
`evidence_request`/`checkpoint_request`/`checkpoint_response`/
`snapshot_manifest`/`snapshot_chunk_request`/
`snapshot_chunk_response`/`light_client_proof_request`/
`light_client_proof_response` — nine, not eight; corrected while
writing this table).

### Handshake

**`Hello`** (`hn-network::handshake`), the entire handshake in one
message each direction (not the RFC's separate `hello`/`capabilities`
messages — a deliberate simplification: real protocols this session
already modeled, like Tendermint's own round structure, benefit from
fewer round trips where nothing forces more, and nothing about
capability negotiation needs a second message once the first already
carries everything):

```text
HelloSigningPayloadV1
  hello_version: u16 (= 1)
  protocol_version: ProtocolVersion
  chain_id: ChainId
  network_id: u16
  node_key: KeyDescriptor        (KeyRole::NodeIdentity)
  supported_channels: set<Channel>
  supported_message_types: set<MessageType>

Hello
  payload: HelloSigningPayloadV1
  signature: SignatureEnvelope   (proves node_key possession)
```

`signing_digest = HASH_PROFILE_0x0001("hnchain.network.hello.v1",
HNCS(HelloSigningPayloadV1))` — a new domain tag, mirroring
`VoteSigningPayloadV1`/`TransactionSigningPayload`'s own
signed-payload-then-signature-envelope shape exactly.

**`HandshakeState`** (pure state machine, no I/O, mirroring
`ConsensusState`'s own shape exactly): `Idle → SentHello → Established`
or `Idle → SentHello → Rejected`. `apply(HandshakeEvent) ->
HandshakeAction`, a deliberately closed, total function over exactly
two events (`SendHello(Hello)`, `ReceiveHello(Hello)`) — checks
`envelope`-level binding first (`chain_id`/`network_id` match this
node's own), then capability overlap (this node's own required channels
and message types are a subset of the peer's advertised
`supported_channels`/`supported_message_types`, and vice versa — ADR-
0018's "unknown required capabilities cause deterministic disconnect,"
implemented here as a plain subset check since no separate "required
vs. optional" capability distinction is decided yet), then verifies
`Hello.signature` against `Hello.payload.node_key` — never resolves
`peer_id`/identity against any store (this crate has no `StateReader`
dependency and none is added; a real node's own peer-reputation/
allowlist policy is out of scope, ADR-0018's own "Local Peer Reputation
As Consensus Input" already rejects treating it as protocol truth
anyway).

**No replay/freshness protection for `Hello` in this pass** — named
explicitly, not silently skipped: binding a handshake signature to a
specific connection attempt (a fresh per-connection challenge) needs an
actual connection/session concept to bind it to, which does not exist
without a real transport (see "Explicitly Not Resolved"). Signing
`HelloSigningPayloadV1` proves the sender holds `node_key`'s private
key; it does not by itself prove the message is fresh rather than a
captured replay of a prior, still-otherwise-valid `Hello`.

### Discovery

**`PeerAddressV1 { peer_id: Digest, network_address: String }`**
(bounded string, `MAX_NETWORK_ADDRESS_LEN`; transport-agnostic on
purpose — no assumption of `host:port` shape, since no transport is
decided) — `PeerAnnounceV1 { peers: Vec<PeerAddressV1> }` (bounded,
`MAX_ANNOUNCED_PEERS`), `PeerRequestV1` (empty payload — "send me what
you know").

**`PeerTable`** (`hn-network::discovery`, pure data structure, no I/O):
a bounded map (`MAX_KNOWN_PEERS`) from `peer_id` to `PeerAddressV1`,
seeded from a caller-supplied static bootstrap list
(`PeerTable::with_bootstrap(peers)`), grown by
`handle_peer_announce(announce) -> usize` (merges new entries, capped,
returns how many were actually new — not blindly trusting everything a
peer claims: duplicate/self entries are dropped), and answers
`build_peer_announce(limit) -> PeerAnnounceV1` (what to tell a peer that
sent `PeerRequest`). No scoring, no eviction policy beyond the bound,
no verification that an announced `network_address` is reachable —
gossip propagation only, exactly the scope chosen.

### Block/Transaction/Vote Propagation

`hn-network::propagation`, each payload paired with the `MessageType`
it travels under:

- `TransactionAnnounceV1 { tx_id: Digest }` / `TransactionRequestV1 {
  tx_id: Digest }` — announce-then-request, per ADR-0018's own
  "Gossip Announcements" pattern.
- `TransactionResponseV1 { envelope: TransactionEnvelope }` — one
  transaction per response for this pass (batching is a real future
  optimization, not attempted here).
- `BlockAnnounceV1 { block_hash: Digest }` / `BlockRequestV1 {
  block_hash: Digest }`.
- `BlockResponseV1 { block_hash: Digest, transactions:
  Vec<TransactionEnvelope> }` — **not a real block**: no header, no
  `state_root`, no justification. The same honest approximation
  `hn_consensus::ConsensusEngine::handle_proposal` already made for the
  identical reason (no concrete `Block`/`BlockHeader` type exists
  anywhere in this codebase) — this is deliberately the exact same
  shape, so a future wiring pass can hand a `BlockResponseV1` straight
  to `handle_proposal` unchanged.
- `ConsensusProposalMessageV1 { block_hash: Digest, transactions:
  Vec<TransactionEnvelope>, justification: Option<QuorumCertificate> }`
  — again, deliberately identical in shape to
  `ConsensusEngine::handle_proposal`'s own three parameters.
- `MessageType::ConsensusVote`'s payload is `hn_state::ConsensusVote`'s
  own canonical bytes directly (no wrapper struct needed — it already
  has `encode`/`decode`).
- `MessageType::QuorumCertificateMessage`'s payload is
  `hn_state::QuorumCertificate`'s own canonical bytes directly, same
  reasoning.

`Vec<TransactionEnvelope>`-carrying payloads use the same hand-rolled
`u32 count || elements` encoding `QuorumCertificate.aggregate_proof`
already established, not `write_list`/`read_list`: `TransactionEnvelope
::decode` can fail with a domain-specific `hn_state::StateError`, which
those generic helpers' `HncsResult`-typed closures cannot express — the
identical justification already used there. `PeerAnnounceV1.peers`
*does* use `write_list`/`read_list` directly: `PeerAddressV1`'s own
decode can only fail with ordinary `HncsError`s (a bounded string, a
fixed digest), so the generic helpers apply cleanly.

## Explicitly Not Resolved

**No real transport.** Nothing here opens a socket. A future pass picks
a concrete transport (and, if async, a runtime) and wires these pure
message/state types to it — a decision this ADR deliberately leaves for
when it is actually made, not guessed at now.

**No connection/session concept**, and therefore no per-connection
replay protection for `Hello` (see "Handshake," above).

**No peer scoring, rate limiting, or eviction policy** beyond
`PeerTable`'s own flat capacity bound. ADR-0018 names all three as real
requirements; none are attempted here.

**No compression or encryption.** V1 implies neither; profiles for
both remain fully open (ADR-0018's own Open Decisions, unchanged).

**Evidence, sync, checkpoint, snapshot, and light-client message
families stay reserved-but-unimplemented** — each blocked on its own
owning ADR (0015/0016/0017) having a concrete object to carry, the same
class of blocker `contract_deploy`/`contract_call` transactions have
always had against HNVM.

**No wiring to `hn_consensus::ConsensusEngine`.** The propagation
payload shapes were deliberately designed to match
`ConsensusEngine`'s own method parameters exactly, but nothing in this
pass actually calls it — that composition (turning a received
`ConsensusProposalMessageV1` into a `handle_proposal` call, turning a
received `ConsensusVote`/`QuorumCertificateMessage` into
`verify_vote`/`handle_quorum_certificate` calls) is real, separate
follow-up work, likely `hn-node`'s own job once a transport exists to
receive anything over.

## Rejected Options

### Picking A Real Transport (Sync `std::net` Or `tokio`) In This Pass

Asked the user; rejected. See "Context" — a consequential,
hard-to-reverse choice that also shapes `hn-node`/`hn-rpc`'s own future
architecture, deliberately deferred to its own dedicated decision.

### Scoping Handshake Authentication To Validators Only

Asked the user; rejected. Would leave exactly the gap ADR-0018's own
RFC text already anticipated ("node identity does not grant validator
authority" implies every node has one) unresolved for another pass, for
no real savings — a generic `NodeIdentity` role costs one registry
value and composes with everything already built for `KeyDescriptor`/
`SignatureEnvelope`.

### A DHT-Style Discovery Protocol

Never seriously considered: ADR-0018 itself never names one (its own
message family list is gossip-shaped — `peer_announce`/`peer_request`
— not Kademlia-shaped), and it would be a large, separate piece of
machinery disproportionate to "basic" P2P.

### Separate `hello`/`capabilities` Messages, Matching The RFC's List Literally

Rejected: nothing about capability negotiation needs two round trips
once the first message already carries everything a peer needs to
decide accept/reject — merging them removes a message family for free
without losing any decided behavior (the RFC's own list is explicitly
non-final, "Initial conceptual message families").

## Security Considerations

Hello replay (see "Explicitly Not Resolved"):

- Risk: a captured `Hello` could be replayed by a party without
  `node_key`'s private key to claim (falsely, to an observer without
  independent verification) that the original signer is present.
- Mitigation: none in this pass — the signature itself still proves
  `node_key` possession at *some* point, just not freshness. A future
  transport-bound challenge closes this; named honestly as unresolved
  rather than assumed away.

Unbounded peer/message growth:

- Risk: a malicious peer floods `PeerAnnounce` entries or claims an
  unbounded `supported_channels`/`supported_message_types` set.
- Mitigation: `PeerTable`'s own `MAX_KNOWN_PEERS` bound,
  `MAX_ANNOUNCED_PEERS` per message, and `Channel`/`MessageType` being
  closed registries (an unrecognized value is a decode error, not an
  admitted unbounded set) already bound this at the type level.

Self-announcement / peer-table poisoning:

- Risk: a peer announces itself, or announces addresses for peer IDs it
  does not control, to bias another node's peer table.
- Mitigation: partial — `PeerTable::handle_peer_announce` drops an
  entry whose `peer_id` matches the local node's own, but does not
  (cannot, without a real connection) verify that an announced
  `network_address` actually belongs to the claimed `peer_id`. Named as
  a real, accepted v1 gap: connecting to a bad address wastes a dial
  attempt, nothing worse, since `Hello`'s own signature still
  authenticates the identity actually reached.

## Compatibility

Purely additive: `hn-network` gains its first real content; `hn-crypto`
gains one new `KeyRole` registry value (no change to any existing
role's meaning or encoding). No changes to `hn-state`/`hn-consensus`
wire formats — `TransactionEnvelope`/`ConsensusVote`/
`QuorumCertificate` are reused exactly as they already encode.

## Open Decisions

- real transport selection (sync vs. async, TCP vs. QUIC vs. both) —
  see "Explicitly Not Resolved"
- connection/session model and `Hello` replay protection
- peer scoring, rate limiting, eviction policy
- compression/encryption profiles
- evidence/sync/checkpoint/snapshot/light-client message schemas
  (blocked on ADR-0015/0016/0017)
- wiring received propagation messages into `ConsensusEngine`
- every other item ADR-0018 itself still lists open, not duplicated
  here

## Related Specifications

- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0018-p2p-protocol-messages.md`
- `docs/adr/ADR-0034-consensus-state-machine-skeleton.md`
- `docs/adr/ADR-0035-wiring-the-consensus-engine-to-hn-state.md`
- `docs/rfc/networking/p2p-protocol-messages.md`
