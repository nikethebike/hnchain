use hn_core::BlockHeight;

/// One request HNChain's `/v1` RPC surface can answer (ADR-0046,
/// "Decided: v1 Endpoint Set") — a Rust-domain call, not parsed HTTP:
/// turning a request line and body into one of these eight variants is
/// the still-unbuilt HTTP server's own job (ADR-0046, "Decided: HTTP
/// Server Is Hand-Written On `std::net`..."), not this type's. Address
/// and transaction-id fields are raw 32-byte values, not the lowercase
/// hex text the wire format uses (ADR-0046, "Decided: Value Encoding
/// Rules") — decoding and validating that hex (`400 invalid_address` on
/// failure) is the same future HTTP layer's job, not duplicated here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RpcCall {
    /// `GET /v1/status`.
    Status,
    /// `GET /v1/blocks/latest`.
    LatestBlock,
    /// `GET /v1/blocks/{height}`.
    BlockAtHeight {
        /// The requested height.
        height: BlockHeight,
    },
    /// `GET /v1/accounts/{address}`.
    Account {
        /// The account's `address_body` (ADR-0003).
        address: [u8; 32],
    },
    /// `GET /v1/transactions/{tx_id}`.
    Transaction {
        /// The transaction's `tx_id` (ADR-0006, "Transaction ID").
        tx_id: [u8; 32],
    },
    /// `POST /v1/transactions`.
    SubmitTransaction {
        /// The raw canonical `TransactionEnvelope` HNCS encoding
        /// (ADR-0046, "Decided: Transaction Submission") — already
        /// hex-decoded by the caller, exactly what
        /// `Mempool::admit`'s own `raw_bytes` parameter expects.
        tx_bytes: Vec<u8>,
    },
    /// `GET /v1/mempool`.
    Mempool,
    /// `GET /v1/validators`.
    Validators,
}
