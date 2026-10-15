# ADR-0046: RPC v1 (HTTP/JSON Read And Submit Surface)

Status: Accepted

Date: 2026-10-14

Version: 0.1.0

Depends On:

- ADR-0000: Foundation (RPC must not bypass state transition validation)
- ADR-0006: Transaction Format
- ADR-0008: Block Format
- ADR-0019: Storage (BlockStore)
- ADR-0038: Genesis Format And Node Daemon Bootstrap
- ADR-0044: Full-Verification Block And State Sync
- ADR-0045: Transactions Through Consensus

Supersedes: None

Referenced By:

- ADR-0047: CLI v1 (`hnchain` talks to a node only through this ADR's
  own HTTP API — never the node's database, never P2P — and its HTTP
  client is a small module added to this crate, `hn-rpc`, so the CLI
  and the integration tests share one implementation)

## Context

`hn-rpc` is a 13-line stub. A running devnet cannot be observed or used
from outside: the only way to inject a transaction is a test-only P2P
handshake client (`hn-node/tests/support`), and the only way to read state
is to parse node log lines (`FINALIZED ... state_root=...`). Every later
component (CLI, wallet, explorer, faucet) needs one stable surface.

What exists and is reused: `StateReader` (account sections),
`BlockStore::{finalized_block_range, latest_finalized_block}`,
`Mempool::{admit, get}`, `ctx.records` (validator records), the node's
single event loop (`NodeEvent` over `mpsc`; no tokio, `std::net` blocking
I/O by project decision), `serde_json` (already a dependency of `hn-node`).

What does not exist: any transaction index, any block-hash index (ADR-0044
deliberately has none), any human-readable address text format with a
checksum (only the 32-byte `address_body`).

## Decision

### Decided: RPC Is Not Consensus

RPC encodings (JSON, hex, field names) are interface conventions, not
protocol. Nothing hashed or signed is defined by this ADR. Canonical
protocol bytes stay HNCS: a submitted transaction is the canonical
`TransactionEnvelope` HNCS encoding, carried as hex. RPC can therefore
change between versions without any consensus effect, and a bug in an RPC
encoder can never fork the chain.

### Decided: Transport — HTTP/1.1 With JSON Bodies, Versioned Path

All endpoints live under `/v1`. UTF-8 JSON, `Content-Type:
application/json`. Chosen over gRPC because every planned consumer
(CLI, browser wallet, explorer) speaks HTTP/JSON natively and the
project's no-async, `std::net` posture makes a gRPC stack
disproportionate. A breaking change creates `/v2`; `/v1` is never
silently reshaped.

### Decided: Value Encoding Rules

- Byte strings (hashes, addresses, keys, signatures, transactions): lowercase
  hex, no `0x` prefix.
- Heights, rounds, counts, timestamps (Unix ms): JSON numbers (u64 values
  stay below 2^53 for any plausible chain lifetime; stated, not assumed
  silently).
- Amounts and any u128 quantity (balances, stakes): **decimal strings in
  hnits**, never JSON numbers (JavaScript consumers lose precision above
  2^53; HNCOIN's 1,000,000,000 × 10^9 hnit supply exceeds it).
- No floating point anywhere.
- Every response that reflects state carries the `height` (and
  `block_hash`) it was read at.

### Decided: Addresses In v1 Are Raw `address_body` Hex

An account is addressed by its 64-hex-character 32-byte `address_body`
(ADR-0003). A human-readable text encoding with a checksum (the roadmap's
"address checksum/validation") does not exist yet; RPC does not invent one.
Introducing it is a separate wallet-facing ADR, after which `/v1` may
accept it additionally. A malformed address (wrong length, non-hex) is a
`400 invalid_address`.

### Decided: v1 Endpoint Set

| Endpoint | Purpose |
| --- | --- |
| `GET /v1/status` | `chain_id`, `network_id`, `genesis_hash`, `protocol_version`, `latest_height`, `latest_block_hash`, `latest_timestamp`, `syncing` (bool), `peer_count`, `mempool_size` |
| `GET /v1/blocks/latest` | latest finalized block (see block shape) |
| `GET /v1/blocks/{height}` | finalized block at height; `404 block_not_found` if above latest |
| `GET /v1/accounts/{address}` | `exists`, `balance` (string, hnits), `nonce`, read at `height` |
| `GET /v1/transactions/{tx_id}` | `status`: `finalized` (with `height`, `index`, `receipt`) \| `pending` (in mempool) \| `unknown` |
| `POST /v1/transactions` | submit one transaction (below) |
| `GET /v1/mempool` | `size`, `capacity` (counts only; no tx listing in v1) |
| `GET /v1/validators` | the validator records this node currently holds: address, status, bonded stake (string) |

Deliberately **not** in v1: block lookup by hash (no hash index exists;
adding one is an ADR-0044 storage change), block ranges/pagination,
transaction listing by account, event subscriptions/WebSocket, any
administrative method, any state-proof endpoint. Each is named as future
work; explorer-grade queries belong to a separate indexer, not to the
node's RPC.

**Block shape.** Header fields (all `BlockHeader` fields, hex/number per the
rules above), `tx_ids` (list), `receipts` (list of `{tx_id, status}`),
`commit_qc` summary (`height`, `round`, `signer_count`). Full transaction
bytes are not embedded (bounded response size: at most 10,000 tx ids);
each is fetched through `/v1/transactions/{tx_id}`.

### Decided: Transaction Submission

`POST /v1/transactions` with body `{"tx": "<hex of canonical TransactionEnvelope>"}`.
The node runs the existing `Mempool::admit` pipeline (ADR-0045) — RPC gets
no cheaper path (ADR-0000, "Validation Before Mutation"). On success:
`202 {"tx_id": "...", "status": "accepted"}` and the transaction is
announced to peers exactly as a P2P-admitted one. `accepted` means
*admitted to this node's mempool*, never *finalized*; the response says so
in its schema documentation and clients poll `/v1/transactions/{tx_id}`.
On failure: `422` with the admission reason mapped to a stable code
(`tx_decode_failed`, `tx_chain_mismatch`, `tx_bad_signature`,
`tx_outside_validity_window`, `tx_would_fail`, `tx_duplicate`,
`mempool_full`). Internal error details (paths, debug strings) are never
returned.

### Decided: Transaction Index

`GET /v1/transactions/{tx_id}` for a finalized tx requires a
`tx_id -> (height, index)` lookup. A `tx_index` table is added to the
storage backend and written **in the same atomic transaction** as the
`FinalizedBlock` and state writes (ADR-0044's atomicity rule extended by
one table). The index is derived data, rebuildable by replaying
`FinalizedBlock`s; its absence never affects consensus. Pending status
comes from `Mempool::get`.

### Decided: Architecture And Crate Direction

The node's `Ctx` (including the store) is owned by the event loop; RPC does
not share the store or open a second handle. Instead:

- `hn-rpc` defines the **request/response types** (`RpcCall`, replies, error
  codes) and the HTTP server (parsing, limits, JSON). It depends only on
  `hn-core` and `serde_json` — **not** on `hn-node`. (Today `hn-rpc`'s
  `Cargo.toml` lists `hn-node`; that edge is reversed, because `hn-node`
  contains the daemon `main.rs` and must be able to start the server.
  Dependency direction: `hn-node → hn-rpc`.)
- `hn-node` gains a `NodeEvent::Rpc { call, reply }` variant. The event loop
  services the call against its own state and answers through a bounded
  one-shot channel. Reads are therefore consistent snapshots at a stated
  height with no locking; submit goes through the same admit path as P2P.
- Each call is O(1) or O(bounded block size) work inside the loop. There
  are no range queries in v1 precisely so a client cannot make the
  consensus loop do unbounded work.
- If the loop does not answer within a deadline (devnet default 2 s), the
  server returns `503 node_busy`. Slow RPC must degrade RPC, never
  consensus.

### Decided: HTTP Server Is Hand-Written On `std::net`, In A Deliberately Tiny Subset

The server is written in `hn-rpc` on `std::net` blocking I/O, matching the
project's no-async, no-heavy-dependency posture. A hand-written parser on
a network-facing port is the riskiest part of this ADR, so the accepted
HTTP surface is a **strict subset chosen to remove whole bug classes**
rather than to be general:

- **One request per connection.** Every response carries `Connection:
  close`; no keep-alive, no pipelining. This removes request smuggling and
  desync by construction.
- **Methods:** `GET` and `POST` only; anything else is `405`.
- **Version:** `HTTP/1.1` only.
- **Request target:** origin-form only (`/v1/...`), at most 2 KiB, matched
  exactly against the route table. Any `%` (no percent-decoding is ever
  needed: ids are hex) or `?` (no query parameters exist in v1) is `400`.
  Absolute-form and authority-form are `400`.
- **Line endings:** CRLF required; bare CR or LF, NUL and other control
  characters in the header section are `400`. Header folding (obs-fold) is
  `400`. Header names must be valid token characters.
- **Header section** at most 8 KiB and 32 headers.
- **Body:** a `POST` requires exactly one `Content-Length` (decimal digits
  only, no sign, no whitespace, within the body limit). Duplicate or
  conflicting `Content-Length` is `400`. Any `Transfer-Encoding` is `501`
  (no chunked support). `Expect` is `417`. A `GET` with a body is `400`.
- **Responses** always set `Content-Length`, `Content-Type` and
  `Connection: close`, and expose no server name or version.
- **Parsing is a pure function** `parse_request(&[u8]) ->
  Result<ParsedRequest, HttpError>` over an already-bounded buffer, with
  no socket access. Socket handling (accept, bounded read loop, deadlines,
  write) is a thin separate layer. The split exists so the parser can be
  unit-tested and fuzzed directly.
- **Concurrency:** a fixed worker pool and a bounded accept queue
  (implementation constants, documented). Connections beyond capacity are
  closed immediately.
- **Deadlines:** a total deadline for reading one full request (slow-client
  / slowloris defence) in addition to per-read timeouts, and a write
  timeout.
- `#![forbid(unsafe_code)]` (already set for the crate) stays.

If a future consumer needs more of HTTP (keep-alive, chunked uploads,
TLS), the answer is a reverse proxy in front of the node, not growth of
this parser.

### Decided: Security And Resource Limits

- RPC is off by default: no server starts at all unless `--rpc-listen
  <addr>` is given (`NodeConfig.rpc_listen: Option<SocketAddr>`, `None`
  by default). When given, the node binds exactly that address, with
  no implicit loopback fallback of its own — a devnet profile's own
  `--rpc-listen 127.0.0.1:<port>` is the operator's own choice, not
  something the node enforces or defaults to. Exposing it beyond
  loopback is therefore an explicit, visible line in the node's own
  command line, never a silent default; nothing in v1 is privileged,
  so no authentication in v1. If an administrative method is ever
  added it is a separate ADR with authentication and is never on
  `/v1`'s public surface.
- Maximum request body: `2 × MAX_TRANSACTION_SIZE + 1 KiB` (hex doubling,
  plus JSON envelope). Maximum header section and URL length bounded
  (implementation constants, documented).
- Per-connection read timeout; maximum concurrent connections; no
  pipelining or keep-alive beyond what the chosen server supports safely.
- CORS disabled by default.
- Response errors never include stack traces or internal paths.
- Rate limiting of `POST` is **not** solved here; it shares the open P2P
  DoS-hardening item. RPC being off unless explicitly enabled, and the
  operator's own choice of bind address when it is, are the v1
  mitigation.

## Rejected Options

### gRPC / Protobuf Now

Rejected: a second serialization system next to HNCS, a large dependency
tree, no benefit for the first consumers (CLI, browser).

### RPC Reading The Store Directly

Rejected: needs a shared mutable store handle across threads (a second
consistency story for redb access), and an RPC reader could observe a state
the consensus loop is mid-way through changing. The message-passing design
costs a little latency and buys consistent snapshots.

### Block Lookup By Hash In v1

Rejected for now: needs a hash index that ADR-0044 deliberately omitted.
Explorer-style queries belong to an indexer built on `FinalizedBlock`s.

### Returning Full Transaction Bytes Inside The Block Response

Rejected: unbounded response size (up to ~8 MiB hex-doubled) and a larger
work item inside the consensus loop.

### Inventing An Address Text Format Inside RPC

Rejected: address format is protocol-adjacent and must be decided once
(with checksum and versioning), not as a side effect of an interface.

## Security Considerations

- RPC cannot mutate state except by submitting a transaction through
  admission; the validation boundary is unchanged.
- `accepted` is not finality; a client treating it as such is a client bug,
  and the schema says so explicitly.
- Public exposure of an unauthenticated node is the operator's explicit
  choice; the default is closed.
- The event-loop bridge is the main availability risk; the bounded
  deadline and bounded per-call work are the mitigation and must be tested.

## Compatibility

New surface, no prior API. Requires: `tx_index` table (devnet data dirs
without it are re-created, as with ADR-0044), a reversed `hn-rpc`/`hn-node`
dependency edge, and a `rpc_listen` config key.

## Conformance / Test Requirements

- Unit: every endpoint's response schema (golden JSON), each error code,
  amount-as-string rule, malformed address/hex/JSON handling.
- Unit: `tx_index` written atomically with the block (crash test like
  ADR-0044's), and rebuilt-from-replay equals the written one.
- Integration (real processes): submit a transfer over HTTP to one node,
  poll until `finalized`, read both balances over HTTP on a *different*
  node, equal; `GET /v1/status` height advances; `syncing` is `true` on a
  restarting node and then `false`.
- Resource limits: oversized body rejected without reading it all; slow
  client does not stall the event loop; `node_busy` returned under a
  blocked loop.
- Parser (mandatory, because it is hand-written): a table-driven unit
  corpus of known malformed and smuggling-class inputs (duplicate and
  conflicting `Content-Length`, `Content-Length` plus `Transfer-Encoding`,
  bare LF, NUL byte, obs-fold, oversized header and line, absolute-form
  target, `%` and `?` in target, request-line variants, truncated
  requests), each asserting the exact rejection. A randomized no-panic /
  no-over-read test over arbitrary byte input runs in CI. A `cargo-fuzz`
  target over `parse_request` exists in the repository and is run
  periodically (not a CI gate), with findings added to the corpus above.
- Network-level: slowloris client (bytes trickled below the per-read
  timeout) is dropped at the total deadline; connection flood beyond pool
  capacity does not stall accepted work; a closed-early client does not
  panic a worker.
- JSON: malformed or oversized JSON bodies and invalid hex are rejected
  with the documented error codes (no panic).

## Open Decisions

- TLS termination (reverse proxy, not in the node).
- Human-readable address encoding with checksum (separate ADR).
- Block-by-hash lookup and account/transaction history (indexer).
- State proofs and light-client endpoints (ADR-0016 checkpoint work).
- WebSocket / event subscription.
- Authenticated administrative surface.
- POST rate limiting (shared with P2P hardening).

## Related Specifications

- `docs/specs/core/transaction-format.md`
- `docs/specs/core/block-format.md`
