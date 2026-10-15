# ADR-0047: CLI v1 (`hnchain` Operator And Developer Tool)

Status: Accepted

Date: 2026-10-14

Version: 0.1.0

Depends On:

- ADR-0002: Cryptographic Identity
- ADR-0003: Address Format
- ADR-0006: Transaction Format
- ADR-0027: Bootstrap Key
- ADR-0045: Transactions Through Consensus
- ADR-0046: RPC v1

Supersedes: None

Referenced By: None

## Context

`hn-cli` is a 13-line stub. After ADR-0046 a node exposes status, blocks,
accounts, transactions and submission over HTTP/JSON, but nothing a person
can run creates a key, builds a signed transaction, or reads a balance.
The only transaction builder is test support code
(`signed_reserve_transfer`), and the only way to hold a key is a raw seed
in node config (ADR-0038: "a raw seed, not a keystore file; real
key-management hardening is explicitly future work").

The CLI is also the first place a human sees an address, an amount and a
signing prompt, so several decisions that were harmless inside the protocol
become user-facing here: how money is displayed and typed, where secret
keys live, how a mistyped recipient is caught.

Facts established from the code before writing this ADR:

- `Ed25519KeyPair::generate(role)` exists but exposes no accessor for the
  generated secret, so a CLI cannot persist a key it generated through that
  path. `from_seed` exists and its doc comment calls it "intended for tests
  and conformance vectors".
- `hn-crypto` has no zeroization of key material.
- The first transaction from an account must carry `bootstrap_key`
  (ADR-0027, `nonce == 0`); later ones must not.
- `GET /v1/accounts/{address}` returns the **committed** nonce, not a
  mempool-aware one.
- The roadmap lists `hnchain init/start/stop`, but the daemon is the
  separate `hn-node` binary.
- `hn-cli/Cargo.toml` depends on `hn-node` and `hn-rpc`.

## Decision

### Decided: The CLI Is An RPC Client, Never A Node

`hnchain` talks to a node only through the ADR-0046 HTTP API. It does not
open the node's database, does not join P2P, does not start or stop
`hn-node`. Process management (`hnchaind init/start/stop` in the roadmap)
stays with the daemon binary and is out of scope. A transaction the CLI
builds goes through the same `Mempool::admit` pipeline as any other
(ADR-0000, "Validation Before Mutation").

Crate dependencies: `hn-cli` depends on `hn-core`, `hn-crypto`, `hn-state`
(transaction types) and `hn-rpc` (shared request/response types and an HTTP
client). It must **not** depend on `hn-node`; the stub's current edge to
`hn-node` is removed so the CLI never links the daemon, consensus or storage.

### Decided: HTTP Client Lives In `hn-rpc`

A minimal one-request-per-connection HTTP/1.1 client (matching the
server's own contract: `Connection: close`, `Content-Length` bodies, no
chunked, no redirects, no TLS) is moved from `hn-node/tests/support` into
`hn-rpc` as a small client module, so the CLI and the integration tests
share one implementation. Deadlines (connect, total) are mandatory. TLS is
not supported: remote access is through a reverse proxy or an SSH tunnel,
exactly as ADR-0046 says for the server side.

### Decided: v1 Command Set

| Command | Purpose |
| --- | --- |
| `hnchain key generate --out <file>` | create a new account key file; print its address and public key |
| `hnchain key show --key <file>` | print address and public key (never the secret) |
| `hnchain status` | `GET /v1/status` |
| `hnchain balance <address>` / `--key <file>` | balance and nonce |
| `hnchain send --key <file> --to <address> --amount <amount>` | build, sign and submit a transfer |
| `hnchain tx show <tx_id>` | `GET /v1/transactions/{tx_id}` |
| `hnchain block show <height\|latest>` | `GET /v1/blocks/...` |
| `hnchain validators` | `GET /v1/validators` |
| `hnchain mempool` | `GET /v1/mempool` |

Global options: `--rpc <url>` (default `http://127.0.0.1:<devnet port>`
from the profile), `--json` (print the node's JSON response unchanged and
nothing else), `--timeout <seconds>`.

Deliberately **not** in v1: staking, unstaking, validator update,
governance, permission update, key rotation, multisig, deactivation (the
protocol supports these transaction types; each needs its own payload
builder and review, and none is needed to use a devnet), mnemonic/HD
wallets, hardware wallets, contract commands (no HNVM), process management.
`send` is the only transaction type in v1.

Argument parsing is a small hand-written subcommand parser (nine commands,
no dependency), consistent with the project's minimal-dependency posture.
This is a user-interface choice that can change to a library later without
affecting anything else.

### Decided: Amounts — Exact Decimal, 9 Fractional Digits, No Floats

HNCOIN has 9 decimals (atomic unit `hnit`). `--amount` takes a strict
decimal string in HNCOIN (`1`, `1.5`, `0.000000001`) and converts to
`hnit` with integer arithmetic only: at most 9 fractional digits (more is
an error, never rounded), no sign, no exponent, no thousands separators,
`u128` overflow is an error, zero is an error. `--amount-hnit` takes the
integer hnit amount directly for scripts. Every output that shows an amount
shows both forms (`1.5 HNCOIN (1500000000 hnit)`) so nothing depends on the
reader knowing the scale. Floating point is never used anywhere in the CLI.

### Decided: Addresses In v1 Are Raw `address_body` Hex

The CLI prints and accepts the 64-hex-character `address_body`, exactly
as the RPC does. A human-readable text format with a checksum does not
exist (see ADR-0046) and the CLI does not invent one. Mitigations for
mistyped recipients: the CLI validates length and hex strictly; `send`
prints the full recipient address and requires confirmation (below); a
recipient that has no account (`exists: false`) produces an explicit
warning in the confirmation summary (a transfer to a fresh address is legal
but a typo looks exactly the same). When an address text encoding with a
checksum is decided (a separate ADR), the CLI adds it for input and
output; hex stays accepted. Nothing persisted by the CLI embeds addresses
in the future format, so deferring costs nothing structural.

### Decided: Key Files — Plain Seed File, Owner-Only, Devnet/Testnet Grade

`key generate` writes a UTF-8 text file:

```
hnchain-key-v1
role: account-signing
seed: <64 lowercase hex characters>
```

- The seed comes from the operating system CSPRNG. Because
  `Ed25519KeyPair::generate` exposes no secret, `hn-crypto` gains a small
  API for this (for example `generate_seed() -> [u8; 32]`, then
  `from_seed`), and the `from_seed` doc comment is corrected so it no
  longer implies seed-based construction is unfit for real keys. The
  secret is never produced by CLI-local randomness code.
- Created with `create_new` (never overwrites an existing file) and mode
  `0600` on Unix. On load the CLI **refuses** a key file readable by group
  or others, with an error naming the file and the fix. On platforms
  without Unix permissions the CLI prints a warning that it cannot enforce
  file protection.
- The secret is only ever read from a file path. It is never accepted on
  the command line or through an environment variable (both leak through
  process listings and shell history), and is never printed, logged or
  included in an error message.
- Seed material is held in memory only as long as signing needs it.
  Zeroization is **not** guaranteed: `hn-crypto` does not zeroize key
  material today. This is recorded as a known limitation, not hidden.
- This format is explicitly **not** custody-grade. No passphrase
  encryption, no OS keychain, no recovery phrase. It is adequate for
  devnet and testnet tokens. An encrypted keystore (KDF, authenticated
  encryption, its dependency audit) is a required separate ADR before any
  key controls mainnet value, and `hnchain` prints a one-line reminder on
  `key generate` that the file is unencrypted.

### Decided: Building And Signing A Transfer

`send` performs, in order, and aborts on the first failure:

1. Load and validate the key file; derive the sender address from the
   account-signing key and the network id.
2. `GET /v1/status`: read `chain_id`, `network_id`, `latest_height`,
   `genesis_hash`. Compare against the profile's pinned values (below);
   a mismatch aborts before anything is signed.
3. `GET /v1/accounts/{sender}`: read `balance` and committed `nonce`.
4. Choose the nonce: `--nonce N` if given, otherwise the committed
   nonce. A transaction with nonce 0 includes `bootstrap_key` (ADR-0027);
   others do not.
5. Build the `TransactionEnvelope` (Transfer, native HNCOIN,
   `asset_id: None`), canonical HNCS encoding, signing digest, Ed25519
   signature. No transaction-format logic is reimplemented in the CLI; it
   calls the same `hn-state` types used by the node and tests.
6. Print a summary (network, sender, recipient, amount in both units,
   fee limit, nonce) and require confirmation: interactive `y/N` on a
   terminal; non-interactive use must pass `--yes` or the command fails
   rather than assume consent.
7. `POST /v1/transactions`. On `202`, print the `tx_id`. `accepted` is
   printed as "admitted to the node's mempool", never as "sent" or
   "confirmed".
8. With `--wait`, poll `GET /v1/transactions/{tx_id}` until `finalized`
   or `--wait-timeout` elapses; exit code distinguishes the outcomes.

**Nonce limitation.** The committed nonce does not count this account's
transactions still waiting in a mempool, so two quick `send` invocations
from one account can both use the same nonce and one will be rejected
or dropped. v1 documents this and offers `--nonce` as the manual
override. A mempool-aware nonce (a sender query in RPC v2) is future work,
not simulated client-side.

**Fee limit and validity window.** Fees are not deducted yet
(ADR-0030) and the fee model is an undecided economic parameter, so the
CLI must not silently invent policy. `--fee-limit` is an explicit option;
its default is a named constant in the CLI profile for the devnet,
documented as a **placeholder, not a protocol value** and printed in the
confirmation summary. The validity window defaults to
`max_height = latest_height + VALIDITY_WINDOW_BLOCKS` (a CLI-local
constant, also documented as non-protocol) so an unincluded transaction
cannot sit valid forever; `--no-expiry` removes it. Both defaults are
reviewed again when the fee model exists.

### Decided: Network Pinning Via A Profile

A profile (a small config file, `--profile <name>`, default `devnet`)
pins `chain_id`, `network_id`, `genesis_hash`, the default RPC URL, and the
fee/window defaults. The CLI refuses to sign when `GET /v1/status` disagrees
with the pinned network. This stops a mistyped or malicious RPC endpoint
from getting a transaction signed for the wrong network (the signature
already binds `chain_id`/`network_id`; pinning makes the wrong-network case
a loud client-side error instead of a transaction that merely fails later).
The CLI ships a `devnet` profile only; no mainnet or testnet profile exists
until those networks do.

### Decided: Output, Exit Codes, Errors

Human-readable output by default, one fact per line, amounts in both
units. `--json` prints exactly the node's JSON response body and nothing
else, so scripts can rely on the RPC schema. Errors go to stderr, one line,
with the stable RPC error code when there is one. Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | success |
| 1 | usage error or invalid input (nothing was sent) |
| 2 | could not reach or understand the node (connect, timeout, bad response) |
| 3 | the node rejected the request (4xx/422 with an error code) |
| 4 | `--wait` timed out before the transaction finalized |

Server-supplied strings are printed escaped so a hostile endpoint cannot
inject terminal control sequences.

## Rejected Options

### CLI Reads The Node's Database Directly

Rejected: two writers or readers on one redb file, and it bypasses the
node's consistent-snapshot guarantee (ADR-0046).

### Encrypted Keystore In v1

Rejected for v1, required before mainnet. It needs a KDF and authenticated
encryption dependency audit and a recovery story; doing it inside the
CLI ADR would stall the devnet tool on a security design that deserves its
own review. The plain file with enforced permissions is honest about what
it is.

### Secret Via Flag Or Environment Variable

Rejected: visible in process listings, shell history and crash reports.

### Inventing An Address Text Format In The CLI

Rejected for the same reason as in ADR-0046: it is protocol-adjacent and
permanent.

### Mnemonic / HD Wallet In v1

Rejected: a separate key-derivation design (and a library decision) with
no devnet need.

### Client-Side Mempool-Aware Nonce

Rejected: guessing pending state from outside invents behavior the node
does not guarantee; needs a real RPC query.

## Security Considerations

- The key file is the only secret. Its protection is file permissions and
  the user's own machine; the documentation says so plainly.
- The confirmation step and network pinning are the main defenses against
  user error and a hostile endpoint; `--yes` exists for scripts and is the
  user's explicit consent.
- The CLI never sends a secret over the network, and RPC is unauthenticated
  plain HTTP: a non-loopback endpoint exposes transaction contents (not
  secrets) to anyone on the path. Documented; use a tunnel or proxy.
- A compromised node can lie about balances and nonce. It cannot forge a
  transaction the user did not sign; it can cause the user to sign
  something with a wrong nonce or fee that then fails. Light-client proofs
  (not in v1) are the eventual defense.

## Compatibility

New tool. Requires `hn-crypto` seed-generation API, the RPC client module in
`hn-rpc`, and removal of `hn-cli`'s dependency on `hn-node`.

## Conformance / Test Requirements

- Unit: decimal amount parsing (every accepted form; rejection of `1.`,
  `.5`, `1e3`, `-1`, `1,5`, more than 9 fractional digits, overflow, zero),
  and hnit-to-display round-trips, including `u128` extremes.
- Unit: key file creation (refuses overwrite, mode 0600), loading
  (refuses group/other-readable, malformed header, wrong length, uppercase
  or non-hex seed), and that no code path prints the seed.
- Unit: transaction construction equals a fixed oracle vector: same
  seed, recipient, amount, nonce, validity window produce byte-identical
  signed envelope bytes (Ed25519 is deterministic), cross-checked against
  the node's own decoding and `tx_id`.
- Unit: nonce 0 carries `bootstrap_key`, nonce > 0 does not.
- Unit: network pinning refuses a mismatched `chain_id`, `network_id` or
  `genesis_hash`.
- Unit: terminal-escape sanitization of server strings.
- Integration (real processes): `key generate` creates an account, the
  devnet reserve funds it through a normal transfer, `send --wait` from
  that new account to a second one finalizes; `balance` on a different
  node shows both results; a second `send` with a stale `--nonce` is
  rejected with the matching exit code 3; `--wait` against a stopped chain
  exits 4; an unreachable node exits 2.
- `--json` output equals the RPC response body byte for byte.

## Open Decisions

- Address text encoding with checksum (shared with ADR-0046).
- Encrypted keystore (required before mainnet).
- Fee model and the real default for `--fee-limit`.
- Mempool-aware nonce (RPC v2).
- Remaining transaction types in the CLI (stake, validator update,
  governance, key rotation, multisig, deactivation).
- Zeroization of key material in `hn-crypto`.
- TLS and authenticated remote access.
- Distribution (packaging, reproducible builds).

## Related Specifications

- `docs/specs/core/transaction-format.md`
- `docs/specs/core/cryptographic-identity.md`
