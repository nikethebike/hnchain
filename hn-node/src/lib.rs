#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Node composition layer for HNChain.
//!
//! This crate wires protocol, storage, networking, RPC, configuration, and
//! process lifecycle components through explicit interfaces.
//!
//! [`genesis::GenesisManifest`] (ADR-0038, "Genesis Format And Node
//! Daemon Bootstrap") is this crate's real genesis: a JSON-sourced,
//! HNCS-canonicalized, `genesis_hash`-committed manifest — the initial
//! validator set and the three ADR-0024 HNCOIN allocation accounts,
//! validated against `hn_state`'s own real monetary/staking constants,
//! never invented values. Its `extra_data` field (ADR-0041, "Genesis
//! Document Commitments Via `extra_data`") reuses `BlockBody.
//! extra_data`'s own already-decided bounded-bytes mechanism
//! (ADR-0008) to carry future document commitments (whitepaper hash,
//! ADR set hash, ...) — empty until a real document-commitment
//! procedure is decided (`docs/specs/core/genesis.md` §6, still open).
//! [`node::run`] (ADR-0037, "Multi-Node
//! Consensus Wiring"; ADR-0038) opens a durable
//! `hn_storage::RedbStateStore`, applies genesis on a fresh database (or
//! verifies an existing one still matches, "Decided: DB Init"),
//! connects to its configured peers over real TCP sockets (directed
//! dial by listen-address ordering, completing
//! `hn_network::HandshakeState`'s handshake on every connection before
//! trusting anything on it), and drives `hn_consensus::ConsensusEngine`
//! off real wall-clock round timers and real network-delivered votes/
//! proposals/quorum certificates. [`config::NodeConfig`] configures a
//! real validator's identity (raw key seeds), genesis file, data
//! directory, and peer list from hand-parsed CLI flags or an equivalent
//! `--config` file of the same flags — no CLI-parsing or file-format
//! dependency added.
//!
//! "Stop" is OS-level process termination — no custom signal handler
//! (ADR-0038, "Decided: No Custom Signal Handling"): `RedbStateStore`'s
//! own transactional guarantee already means an interrupted process
//! leaves no corrupted state, and every safe way to catch `Ctrl+C`/
//! `SIGINT` needs either `unsafe` FFI (forbidden here outright) or a new
//! dependency for a purely cosmetic benefit.
//!
//! [`node::run`]'s proposer now builds a real `hn_state::BlockHeader`
//! and computes its own real `block_hash` (ADR-0040, "Real Block Hash
//! In `hn-node`") — replacing the earlier synthetic, non-cryptographic
//! per-round identifier. A connection this node itself dialed is
//! automatically redialed if it later drops (ADR-0039, "Decided:
//! Connection-Drop Reconnection"), and a rejoining node passively
//! catches up to the network's real current height by observing
//! ordinary gossiped vote/certificate/proposal traffic (ADR-0039,
//! "Decided: Passive Height-Observation Catch-Up") — no dedicated sync
//! protocol. Each such jump starts a fresh `ConsensusEngine` with no
//! cached proposal, so a quorum certificate for a block this node
//! never itself saw `Proposal` for cannot be applied; rather than exit
//! the whole process over that one missed height, [`node::run`] logs
//! and skips it (a `SKIPPED finalizing ...` line), leaving the engine
//! to be replaced wholesale by the next, self-healing catch-up jump —
//! found as a real crash via CI's own multi-node restart-recovery test
//! failing under load (never locally), not a hypothetical.
//!
//! A receiving node no longer trusts a proposer's claimed `block_hash`
//! at face value (ADR-0040's own "Explicitly Not Resolved," now
//! closed): [`node::run`] verifies a whole `hn_state::BlockEnvelope`
//! (ADR-0042, "BlockEnvelope And Receiver-Side Header Verification") —
//! recomputing `block_hash` from the received header, authenticating
//! the proposer (`round_proposer` eligibility plus a real signature
//! over that hash from the proposer's own registered consensus key),
//! cross-checking the header against its own body, and — only once
//! this node's own chain position is itself known to be correct, not
//! mid-`maybe_sync_forward`-catch-up — `parent_block_hash` and strict
//! timestamp monotonicity. An embedded `justification` quorum
//! certificate's own signatures are verified the same way a normal
//! gossiped one already was. Any failure is a dropped, logged message,
//! never a crash: externally-triggerable network input must not be
//! able to take this process down, independent of ADR-0038's own
//! "no-panic discipline" at the process boundary.
//!
//! Genesis is now a real block 0 (ADR-0043, "Genesis As A Real Block
//! 0"), not an arbitrary placeholder: [`node::run`] builds the same
//! `hn_state::BlockHeader` from the same `GenesisManifest` every other
//! node independently builds, and seeds its own chain position —
//! `last_block_hash`/`last_timestamp`, trusted from startup — from
//! that real, computed header rather than an `[0; 32]`/`0` sentinel
//! nothing could actually get wrong. The first real consensus-produced
//! block now starts at height 1, not 0 — genesis alone holds height
//! 0, keeping `height = parent.height + 1` true across that boundary
//! too. ADR-0042's own `parent_block_hash` check is therefore
//! meaningful starting from the very first real block, not just from
//! the second one onward.
//!
//! Explicitly out of scope, named rather than guessed at: any RPC/CLI
//! surface, real (non-devnet) validator onboarding and real custody for
//! the three genesis allocation accounts (`docs/specs/core/genesis-
//! security.md`'s own open items), non-validating "full node" mode,
//! general malformed-message decode resilience (every message type,
//! this one included, still propagates a fatal error out of a failed
//! `::decode` — a real, separate, broader hardening gap ADR-0042 does
//! not attempt to close), re-proposing a locked value under a real
//! `justification` on the *proposing* side (this crate's own proposer
//! still always sends `justification: None`), real block/state sync
//! for a future network with non-empty blocks (specified, not yet
//! implemented, ADR-0044 — including this node's own inability to
//! verify `parent_block_hash`/timestamp monotonicity immediately
//! after a passive catch-up jump, ADR-0039), and ADR-0011's real
//! leader-election formula (this crate uses
//! `hn_consensus::round_proposer`, its own explicitly-sanctioned
//! devnet placeholder) — see ADR-0037/ADR-0038/ADR-0039/ADR-0040/
//! ADR-0042's own "Explicitly Not Resolved."

mod config;
mod error;
mod genesis;
mod hex;
mod identity;
mod mempool;
mod node;
mod rpc_bridge;

pub use config::{ConfigError, NodeConfig};
pub use error::{NodeError, NodeResult};
pub use genesis::{
    GENESIS_MANIFEST_VERSION_1, GENESIS_MESSAGE_MAX_LEN, GenesisAccount, GenesisAllocations,
    GenesisError, GenesisManifest, GenesisValidator, MAX_GENESIS_VALIDATORS,
};
pub use mempool::{MEMPOOL_MAX_TXS, Mempool, MempoolError};
pub use node::run;
pub use rpc_bridge::{RpcContext, handle_rpc_call};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
