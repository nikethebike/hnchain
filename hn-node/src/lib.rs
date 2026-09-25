#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Node composition layer for HNChain.
//!
//! This crate wires protocol, storage, networking, RPC, configuration, and
//! process lifecycle components through explicit interfaces.
//!
//! [`node::run`] (ADR-0037, "Multi-Node Consensus Wiring") is this
//! crate's first real content: a genuine `std::net`-based process that
//! opens a durable `hn_storage::RedbStateStore`, connects to its
//! configured peers over real TCP sockets (directed dial by
//! `validator_id`, completing `hn_network::HandshakeState`'s handshake
//! on every connection before trusting anything on it), and drives
//! `hn_consensus::ConsensusEngine` off real wall-clock round timers and
//! real network-delivered votes/proposals/quorum certificates — no
//! in-process shortcut anywhere in the loop. [`config::NodeConfig`]
//! configures one devnet validator's identity, cluster topology, and
//! round-timeout defaults from hand-parsed CLI flags.
//!
//! Explicitly out of scope, named rather than guessed at: any RPC/CLI
//! surface, real (non-devnet) validator onboarding, connection retry
//! after an established link drops, and ADR-0011's real leader-election
//! formula (this crate uses `hn_consensus::round_proposer`, its own
//! explicitly-sanctioned devnet placeholder) — see ADR-0037's own
//! "Explicitly Not Resolved."

mod config;
mod error;
mod genesis;
mod identity;
mod node;

pub use config::{ConfigError, NodeConfig};
pub use error::{NodeError, NodeResult};
pub use node::run;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
