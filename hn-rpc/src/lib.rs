#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! RPC surface boundaries for HNChain.
//!
//! This crate exposes versioned API boundaries without bypassing protocol
//! validation layers.
//!
//! [`RpcCall`]/[`RpcReply`]/[`RpcError`] (ADR-0046, "RPC v1") are the
//! `/v1` surface's own request/response types, as Rust-domain values —
//! not yet HTTP or JSON. Depends only on `hn_core` and `serde_json`
//! (ADR-0046, "Decided: Architecture And Crate Direction": `hn-node`
//! contains the daemon and must be able to start the RPC server this
//! crate will eventually define, so the dependency runs
//! `hn-node -> hn-rpc`, never the reverse). The hand-written `std::net`
//! HTTP server, and the hex/decimal-string JSON wire encoding
//! ADR-0046's own "Decided: Value Encoding Rules" settles, remain
//! future work — these types are what that layer will translate
//! requests into and responses out of, not a replacement for it.

mod call;
mod error;
mod reply;

pub use call::RpcCall;
pub use error::RpcError;
pub use reply::{
    AccountReply, BlockHeaderReply, BlockReply, CommitQcSummary, MempoolReply, ReceiptEntry,
    RpcReply, RpcResult, StatusReply, SubmittedReply, TransactionReply, TransactionStatus,
    ValidatorEntry, ValidatorStatusReply, ValidatorsReply,
};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
