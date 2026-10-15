#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Command-line interface boundary for HNChain.
//!
//! This crate is a client of explicit node and RPC interfaces, not a private
//! protocol-state backdoor. It talks to a node only through ADR-0046's own
//! HTTP API (ADR-0047, "Decided: The CLI Is An RPC Client, Never A Node") —
//! it does not open the node's database, join P2P, or start/stop `hn-node`.
//!
//! [`cli`] is the `hnchain` binary's own argument parser: a small
//! hand-written subcommand parser for the nine `/v1` commands ADR-0047
//! names, with no parsing-library dependency. [`sanitize`] escapes any
//! server-supplied string before it is ever printed, so a hostile or
//! compromised RPC endpoint cannot inject terminal control sequences
//! into this process's own stdout/stderr.
//!
//! Command execution (actually calling the node, loading keys, signing
//! transactions) lands one command at a time in later passes; today
//! only parsing, `--help`, and exit codes exist.

pub mod cli;
pub mod sanitize;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
