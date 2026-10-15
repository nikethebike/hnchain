//! The active network profile — today, a single hardcoded devnet
//! constant good enough for [`crate::keyfile`]'s own address
//! derivation; ADR-0047's own "Decided: Network Pinning Via A
//! Profile" (a real `--profile <name>` config file pinning
//! `chain_id`/`network_id`/`genesis_hash`/the default RPC URL/fee and
//! window defaults, with a refusal to sign against a node whose own
//! `GET /v1/status` disagrees) is a later pass's own job, not yet
//! built here. `NETWORK_ID` is the devnet's own already-established
//! real value (used throughout this workspace's own test fixtures and
//! `hn-node`'s own genesis loader) — not an invented placeholder, in
//! the sense the project's own "no implicit economic values" rule
//! means; it is a protocol/network identifier, never a monetary
//! parameter.

/// The devnet's own `network_id` (ADR-0003).
pub const NETWORK_ID: u16 = 1;
