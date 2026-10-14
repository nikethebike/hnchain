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
//!
//! [`spawn_server`] is the socket layer the parser plugs into: a
//! fixed worker pool reading off one shared, bounded accept queue, a
//! total per-connection read deadline on top of per-read timeouts
//! (slowloris defence), and a write timeout — every response carries
//! `Connection: close` ([`HttpResponse::write_to`]), matching "One
//! request per connection." It answers a successfully-parsed request
//! through the caller-supplied [`RequestHandler`]; routing the eight
//! `/v1` methods to real answers, and the hex/decimal-string JSON wire
//! encoding ADR-0046's own "Decided: Value Encoding Rules" settles,
//! remain future work — today's handler and its error-fallback
//! responses are both deliberately minimal.

mod call;
mod error;
mod parser;
mod reply;
mod response;
mod server;

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
pub use response::HttpResponse;
pub use server::{RequestHandler, ServerConfig, spawn_server};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
