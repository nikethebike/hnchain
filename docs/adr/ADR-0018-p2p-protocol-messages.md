# ADR-0018: P2P Protocol Messages

Status: Proposed

Date: 2026-07-18

Version: 0.1.0

Depends On:

- ADR-0000: Protocol Invariants
- ADR-0002: Cryptographic Identity
- ADR-0004: Canonical Serialization
- ADR-0005: Hash Algorithms
- ADR-0006: Transaction Format
- ADR-0008: Block Format
- ADR-0009: Consensus Architecture
- ADR-0012: Vote Messages And Quorum Certificates
- ADR-0016: Synchronization Checkpoints
- ADR-0017: Light-Client Finality Proofs

Supersedes: None

Referenced By:

- ADR-0036: Basic P2P Networking (closes "final envelope schema"/
  "final channel registry"/"final message type registry"/"handshake
  schema" for a deliberately narrow slice — node discovery, handshake,
  block/transaction/vote propagation — leaving the rest of this ADR's
  own Open Decisions untouched)

## Context

HNChain nodes exchange transactions, blocks, consensus messages, evidence,
checkpoints, snapshots, peer metadata, and light-client proof material through
the peer-to-peer network.

The network layer must be modular and transport-independent. TCP, QUIC, or
future transports may carry the same canonical P2P messages, but transport
framing must not define consensus validity.

P2P messages must be versioned, bounded, authenticated where required, and safe
to reject before expensive processing.

## Decision

HNChain defines a versioned P2P message envelope.

Conceptual envelope:

```text
P2PMessageEnvelope
  envelope_version
  protocol_version
  chain_id
  network_id
  channel
  message_type
  message_id
  capabilities
  compression_profile
  encryption_profile
  payload_length
  payload_hash
  payload
  authentication
```

Message payloads reference canonical protocol objects defined by ADRs and RFCs.

P2P messages may transport consensus objects, but they do not redefine them.

## Normative Rules

### Versioned Envelope

Every P2P message includes `envelope_version` and `protocol_version`.

Nodes must not infer message format from port number, transport protocol, peer
software, payload length, or channel name.

### Chain And Network Binding

Every message that carries chain-specific data must bind to `chain_id` and
`network_id`.

Cross-network replay must be rejected before expensive validation.

### Channel Separation

P2P traffic is separated into logical channels.

Initial conceptual channels:

- `handshake`
- `peer_discovery`
- `transactions`
- `blocks`
- `consensus`
- `evidence`
- `sync`
- `snapshot`
- `light_client`
- `control`

Channels enable prioritization, limits, and backpressure.

### Message Types

Initial conceptual message families:

- handshake and capability exchange
- peer discovery
- transaction announcement
- transaction request and response
- block announcement
- block request and response
- consensus proposal
- consensus vote
- quorum certificate
- evidence announcement
- checkpoint request and response
- snapshot manifest and chunk exchange
- light-client proof request and response
- ping, pong, and disconnect

Each active message type requires a schema, size limit, validation pipeline, and
test vectors.

### Capability Negotiation

Peers must negotiate supported protocol versions and capabilities during
handshake.

Capabilities may include:

- supported transports
- supported compression profiles
- supported encryption profiles
- supported consensus profiles
- supported sync modes
- supported snapshot profiles
- supported light-client proof versions

Unknown required capabilities cause deterministic disconnect or downgrade
behavior.

### Transport Independence

P2P messages must be valid independently of the transport.

Transport-specific fields, connection identifiers, packet numbers, TLS session
state, and local socket metadata are not consensus inputs.

### Authentication

Node authentication must use cryptographic identity descriptors.

Authentication requirements depend on message type:

- consensus messages require validator consensus signatures
- peer identity messages require node identity authentication
- transaction messages carry transaction authorization in payload
- public data requests may be unauthenticated but rate-limited

### Compression

Compression is negotiated by capability.

Compressed bytes are transport payloads. Consensus hashes are computed over
canonical uncompressed protocol objects, not compressed network frames.

### Encryption

Peer connections should be encrypted when supported by the selected transport
profile.

Encryption protects transport confidentiality and integrity, but consensus
validity still requires canonical object verification.

### Cheap Rejection

Nodes must reject invalid messages before expensive work where possible.

Initial rejection stages:

```text
bytes
  -> frame limit
  -> envelope decode
  -> version check
  -> chain and network check
  -> channel and type check
  -> payload length limit
  -> payload hash check
  -> authentication precheck
  -> payload decode
  -> protocol object validation
```

### Gossip Announcements

Large objects should be announced by identifier before full transfer.

Conceptual flow:

```text
announce hash
  -> peer checks need
  -> request object
  -> receive object
  -> verify object
```

This reduces bandwidth and DoS exposure.

## Rejected Options

### Consensus Objects Defined By Network Frames

Rejected because consensus validity must be independent of transport encoding.

### Unversioned P2P Messages

Rejected because the network protocol must evolve without ambiguous parsing.

### Gossip Full Objects To Every Peer By Default

Rejected because it wastes bandwidth and increases DoS exposure.

### Local Peer Reputation As Consensus Input

Rejected because reputation is local networking policy, not consensus truth.

### Compression Before Consensus Hashing

Rejected because compression algorithms and settings can vary across nodes.

## Alternatives Considered

### TCP-Only Network

Advantages:

- simple operational model
- widely supported
- mature tooling

Disadvantages:

- locks HNChain to one transport assumption
- harder future migration
- less flexibility for multiplexing and mobility

### QUIC-First Network

Advantages:

- built-in multiplexing
- modern connection migration
- good fit for independent channels

Disadvantages:

- operational complexity
- implementation maturity varies by language
- may be harder for some infrastructure environments

### Transport Abstraction

Advantages:

- preserves long-term replaceability
- allows TCP and QUIC profiles
- keeps protocol messages stable

Disadvantages:

- more specification work
- more test matrix complexity
- requires strict boundary discipline

## Security Considerations

DoS by oversized messages:

- Risk: attackers force expensive memory allocation or decoding.
- Mitigation: frame limits, payload limits, and cheap rejection.

Malformed object spam:

- Risk: attackers flood invalid transactions, blocks, or votes.
- Mitigation: staged validation, peer scoring, rate limits, and bans.

Replay across networks:

- Risk: messages from another network are accepted.
- Mitigation: chain ID and network ID binding.

Compression bombs:

- Risk: compressed payload expands to excessive memory.
- Mitigation: negotiated profiles and decompressed-size limits.

Authentication confusion:

- Risk: node identity is confused with validator consensus authority.
- Mitigation: separate node keys, validator keys, and signing purposes.

Consensus message delay:

- Risk: low-priority traffic blocks consensus traffic.
- Mitigation: channel priorities and backpressure.

Eclipse attacks:

- Risk: attacker controls a node's peer view.
- Mitigation: peer diversity, discovery rules, connection limits, and monitoring.

## Compatibility

Adding a message type can be compatible only if:

- message type is registered
- schema is versioned
- size limits are defined
- unsupported peers reject or ignore it deterministically
- capability negotiation is updated

Changing envelope fields, authentication semantics, or required capabilities is
a network protocol change and may require staged rollout.

Consensus object compatibility remains governed by the corresponding core or
consensus specifications.

## Open Decisions

- initial transport profiles (ADR-0036 deliberately does not pick one —
  pure protocol logic only, no real sockets)
- envelope schema — resolved for V1, ADR-0036 (compression/encryption
  fields deliberately omitted, not finalized)
- channel registry — resolved for V1, ADR-0036 (5 of 10 channels
  implemented; evidence/sync/snapshot/light_client reserved)
- message type registry — resolved for V1, ADR-0036 (15 of ~24 message
  types implemented; the rest reserved)
- handshake schema — resolved for V1, ADR-0036 (`Hello`, merging the
  RFC's own separate `hello`/`capabilities` messages)
- capability negotiation schema — resolved for V1, ADR-0036 (a plain
  subset check over `supported_channels`/`supported_message_types`, no
  required-vs-optional distinction yet)
- node identity format — resolved, ADR-0036 (`KeyRole::NodeIdentity` +
  `peer_id`, deliberately not network-bound)
- compression profiles
- encryption profiles
- peer scoring policy
- rate limit policy
- gossip fanout strategy — partially resolved, ADR-0036 (static
  bootstrap + `peer_announce`/`peer_request` gossip; no fanout
  algorithm decided)
- block propagation strategy — partially resolved, ADR-0036
  (announce/request/response for a bare transaction list under a block
  hash, not a real block — see ADR-0036's own "Explicitly Not
  Resolved")
- sync packet formats
- snapshot packet formats
- P2P test vector suite — this pass added unit/oracle tests for the
  implemented subset, not a formal conformance vector file

## Related Specifications

- `docs/rfc/networking/p2p-protocol-messages.md`
