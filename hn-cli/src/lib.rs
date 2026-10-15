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
//! into this process's own stdout/stderr. [`amount`] is the strict
//! decimal HNCOIN/`hnit` grammar `--amount`/`--amount-hnit` parse
//! against. [`keyfile`] is the plain, owner-only-permissions key file
//! format `key generate`/`key show` read and write — explicitly not
//! custody-grade (ADR-0047, "Decided: Key Files"). [`profile`] is
//! today's one hardcoded devnet constant, standing in for the real
//! network-pinning profile a later pass builds.
//!
//! Command execution for `send` and the read-only `/v1` commands
//! (actually calling the node, signing transactions) lands one
//! command at a time in later passes.

pub mod amount;
pub mod cli;
pub mod hex;
pub mod keyfile;
pub mod profile;
pub mod rpc;
pub mod rpc_url;
pub mod sanitize;
pub mod send;
pub mod transfer;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
