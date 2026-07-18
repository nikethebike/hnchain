# HNChain Networking RFC: P2P Protocol Messages

Status: Proposed

Version: 0.1.0

Depends On:

- `docs/adr/ADR-0000-protocol-invariants.md`
- `docs/adr/ADR-0002-cryptographic-identity.md`
- `docs/adr/ADR-0004-canonical-serialization.md`
- `docs/adr/ADR-0005-hash-algorithms.md`
- `docs/adr/ADR-0006-transaction-format.md`
- `docs/adr/ADR-0008-block-format.md`
- `docs/adr/ADR-0009-consensus-architecture.md`
- `docs/adr/ADR-0012-vote-messages-and-quorum-certificates.md`
- `docs/adr/ADR-0016-synchronization-checkpoints.md`
- `docs/adr/ADR-0017-light-client-finality-proofs.md`
- `docs/adr/ADR-0018-p2p-protocol-messages.md`

## 1. Purpose

This RFC defines the conceptual P2P message model for HNChain networking.

It specifies envelope fields, channels, message families, capability
negotiation, transport boundaries, authentication requirements, and security
limits.

## 2. Scope

This RFC defines:

- P2P message envelope requirements
- channel registry requirements
- message family requirements
- capability negotiation requirements
- transport independence
- authentication boundaries
- compression and encryption boundaries
- cheap rejection pipeline

This RFC does not define:

- final TCP profile
- final QUIC profile
- final peer discovery protocol
- final gossip fanout algorithm
- final rate limit values
- final node implementation API

## 3. Message Envelope

Conceptual structure:

```text
P2PMessageEnvelopeV1
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

Envelope fields are HNCS-encoded unless the active transport profile defines a
lower-level frame wrapper.

Consensus objects inside `payload` remain encoded by their own canonical
schemas.

## 4. Channels

Initial conceptual channels:

```text
handshake
peer_discovery
transactions
blocks
consensus
evidence
sync
snapshot
light_client
control
```

Every channel must define:

- priority
- size limits
- allowed message types
- authentication requirements
- rate limits
- backpressure behavior

## 5. Message Families

Initial conceptual message families:

```text
hello
capabilities
peer_announce
peer_request
transaction_announce
transaction_request
transaction_response
block_announce
block_header_request
block_body_request
block_response
consensus_proposal
consensus_vote
quorum_certificate
evidence_announce
evidence_request
checkpoint_request
checkpoint_response
snapshot_manifest
snapshot_chunk_request
snapshot_chunk_response
light_client_proof_request
light_client_proof_response
ping
pong
disconnect
```

The final registry must assign stable numeric identifiers.

## 6. Capability Negotiation

Handshake must negotiate:

- protocol version
- chain ID and network ID
- supported channels
- supported message versions
- supported compression profiles
- supported encryption profiles
- supported sync modes
- supported snapshot profiles
- supported light-client proof versions
- peer identity profile

Peers must disconnect or downgrade deterministically when required capabilities
do not overlap.

## 7. Authentication

Authentication is message-specific.

Conceptual rules:

- consensus payloads verify validator signatures inside the payload
- node control messages verify node identity authentication
- transaction payloads verify transaction signatures inside the payload
- public requests may be unauthenticated but rate-limited

Node identity does not grant validator authority.

Validator authority does not grant unrestricted network trust.

## 8. Compression And Encryption

Compression and encryption are negotiated transport features.

Consensus IDs, block hashes, transaction IDs, state roots, and proof hashes are
computed over canonical protocol bytes, not compressed or encrypted frames.

Implementations must bound decompressed payload size.

## 9. Rejection Pipeline

Conceptual pipeline:

```text
incoming_bytes
  -> frame limit
  -> envelope decode
  -> version check
  -> chain and network check
  -> channel check
  -> message type check
  -> payload length check
  -> payload hash check
  -> authentication precheck
  -> payload decode
  -> protocol object validation
  -> routing
```

Implementations may reorder independent cheap checks, but must not perform
expensive validation before basic bounds are enforced.

## 10. Announcement Pattern

Large objects should use announce/request/response.

Conceptual flow:

```text
announce(object_id)
  -> request(object_id)
  -> response(object_bytes)
  -> verify(object_bytes)
```

Object IDs must be derived from canonical protocol bytes using the relevant hash
profile.

## 11. Consensus Message Propagation

Consensus messages use the `consensus` channel.

They must receive higher priority than ordinary transaction gossip during
congestion.

The network layer may prioritize delivery, but consensus validity is determined
only by consensus specifications.

## 12. Security Requirements

Implementations must reject:

- unknown required envelope versions
- unsupported protocol versions
- messages for another chain or network
- unsupported channels
- unsupported message types
- oversized frames
- oversized payloads
- payload hash mismatches
- unsupported compression profiles
- decompressed payloads above limit
- malformed authentication data
- consensus payloads with invalid canonical encoding

Implementations must bound:

- frame size
- payload size
- decompressed size
- envelope decode time
- authentication precheck time
- per-peer queues
- per-channel queues
- outstanding requests

## 13. Test Vectors

The accepted version must include test vectors for:

- valid envelope
- wrong chain rejection
- wrong network rejection
- unsupported version rejection
- unsupported channel rejection
- payload hash mismatch
- oversized payload rejection
- compression limit rejection
- consensus vote transport
- block announcement flow
- checkpoint response flow
- light-client proof response flow

Test vectors are mandatory before production implementation.

## 14. Open Decisions

- final envelope schema
- final numeric channel registry
- final numeric message registry
- final handshake schema
- final capability negotiation behavior
- final peer identity proof
- final TCP profile
- final QUIC profile
- final compression profiles
- final encryption profiles
- final rate limits
- final gossip fanout
- final sync request and response schemas
