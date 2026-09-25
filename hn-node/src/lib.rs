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
//! never invented values. [`node::run`] (ADR-0037, "Multi-Node
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
//! Explicitly out of scope, named rather than guessed at: any RPC/CLI
//! surface, real (non-devnet) validator onboarding and real custody for
//! the three genesis allocation accounts (`docs/specs/core/genesis-
//! security.md`'s own open items), non-validating "full node" mode,
//! connection retry after an established link drops, and ADR-0011's
//! real leader-election formula (this crate uses
//! `hn_consensus::round_proposer`, its own explicitly-sanctioned devnet
//! placeholder) — see ADR-0037/ADR-0038's own "Explicitly Not
//! Resolved."

mod config;
mod error;
mod genesis;
mod hex;
mod identity;
mod node;

pub use config::{ConfigError, NodeConfig};
pub use error::{NodeError, NodeResult};
pub use genesis::{
    GENESIS_MANIFEST_VERSION_1, GENESIS_MESSAGE_MAX_LEN, GenesisAccount, GenesisAllocations,
    GenesisError, GenesisManifest, GenesisValidator, MAX_GENESIS_VALIDATORS,
};
pub use node::run;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
