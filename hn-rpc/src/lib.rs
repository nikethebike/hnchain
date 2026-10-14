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
//! `hn-node -> hn-rpc`, never the reverse).
//!
//! [`parse_request`] (ADR-0046, "Decided: HTTP Server Is Hand-Written
//! On `std::net`, In A Deliberately Tiny Subset") is that server's
//! own parser: a pure function over an already-in-memory buffer, no
//! socket access, so it can be unit-tested (and later fuzzed)
//! directly against a table of malformed/smuggling-class inputs.
//! Everything else that subset names — accept/read/write socket
//! handling with real deadlines, routing the eight `/v1` methods,
//! and the hex/decimal-string JSON wire encoding ADR-0046's own
//! "Decided: Value Encoding Rules" settles — remains future work.

mod call;
mod error;
mod parser;
mod reply;

pub use call::RpcCall;
pub use error::RpcError;
pub use parser::{
    HttpError, HttpMethod, MAX_HEADER_COUNT, MAX_HEADER_SECTION_LEN, MAX_REQUEST_TARGET_LEN,
    ParsedRequest, parse_request,
};
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
