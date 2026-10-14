use std::fmt;

/// Why an [`crate::RpcCall`] could not be answered successfully
/// (ADR-0046, "Decided: v1 Endpoint Set" / "Decided: Transaction
/// Submission") — every variant is one of that ADR's own named stable
/// wire error codes, returned here as a closed Rust enum rather than a
/// bare string so a caller can match on it instead of comparing text.
/// [`Self::code`] is the exact wire spelling the eventual HTTP layer's
/// JSON error body uses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RpcError {
    /// `GET /v1/accounts/{address}` with a malformed address (wrong
    /// length or non-hex) — constructed by the HTTP layer, not
    /// anything in this crate today, since [`crate::RpcCall::Account`]
    /// already carries a structurally-valid `[u8; 32]`.
    InvalidAddress,
    /// `GET /v1/blocks/{height}` above the latest finalized height.
    BlockNotFound,
    /// A submitted transaction's bytes did not decode as a canonical
    /// `TransactionEnvelope` (`Mempool::admit`'s step 1).
    TxDecodeFailed,
    /// A submitted transaction's `chain_id`/`network_id` does not
    /// match this node's own (`Mempool::admit`'s step 2).
    TxChainMismatch,
    /// A submitted transaction's signature verification failed
    /// (`Mempool::admit`'s step 3).
    TxBadSignature,
    /// A submitted transaction's `validity_window` does not cover the
    /// next height (`Mempool::admit`'s step 4).
    TxOutsideValidityWindow,
    /// A submitted transaction would abort block application in a dry
    /// run (`Mempool::admit`'s step 5).
    TxWouldFail,
    /// This exact `tx_id` is already admitted (`Mempool::admit`'s step
    /// 6).
    TxDuplicate,
    /// The mempool is at capacity.
    MempoolFull,
    /// The event loop did not answer within its deadline (ADR-0046,
    /// "Decided: Architecture And Crate Direction") — constructed by
    /// the HTTP layer on a timed-out reply channel, not by anything in
    /// this crate today.
    NodeBusy,
    /// No route matches this request's own method and target at all
    /// (ADR-0046's own "v1 Endpoint Set" names exactly eight; anything
    /// else, including a known path with the wrong method, is this) —
    /// not one of ADR-0046's own explicitly-named codes, but the only
    /// sensible answer to a request this surface simply does not
    /// define.
    NotFound,
}

impl RpcError {
    /// This error's stable wire code, exactly as ADR-0046 names it
    /// (or, for [`Self::NotFound`], this crate's own small addition to
    /// that registry).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidAddress => "invalid_address",
            Self::BlockNotFound => "block_not_found",
            Self::TxDecodeFailed => "tx_decode_failed",
            Self::TxChainMismatch => "tx_chain_mismatch",
            Self::TxBadSignature => "tx_bad_signature",
            Self::TxOutsideValidityWindow => "tx_outside_validity_window",
            Self::TxWouldFail => "tx_would_fail",
            Self::TxDuplicate => "tx_duplicate",
            Self::MempoolFull => "mempool_full",
            Self::NodeBusy => "node_busy",
            Self::NotFound => "not_found",
        }
    }

    /// The HTTP status this error maps to (ADR-0046, "Decided:
    /// Transaction Submission" — every admission failure is `422`,
    /// distinct from the generic `400`/`404` the rest of this registry
    /// uses).
    #[must_use]
    pub const fn status_code(self) -> u16 {
        match self {
            Self::InvalidAddress => 400,
            Self::BlockNotFound | Self::NotFound => 404,
            Self::TxDecodeFailed
            | Self::TxChainMismatch
            | Self::TxBadSignature
            | Self::TxOutsideValidityWindow
            | Self::TxWouldFail
            | Self::TxDuplicate
            | Self::MempoolFull => 422,
            Self::NodeBusy => 503,
        }
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.code())
    }
}

impl std::error::Error for RpcError {}

#[cfg(test)]
mod tests {
    use super::RpcError;

    #[test]
    fn code_matches_the_adrs_own_wire_spelling() {
        assert_eq!(RpcError::InvalidAddress.code(), "invalid_address");
        assert_eq!(RpcError::BlockNotFound.code(), "block_not_found");
        assert_eq!(RpcError::TxDecodeFailed.code(), "tx_decode_failed");
        assert_eq!(RpcError::TxChainMismatch.code(), "tx_chain_mismatch");
        assert_eq!(RpcError::TxBadSignature.code(), "tx_bad_signature");
        assert_eq!(
            RpcError::TxOutsideValidityWindow.code(),
            "tx_outside_validity_window"
        );
        assert_eq!(RpcError::TxWouldFail.code(), "tx_would_fail");
        assert_eq!(RpcError::TxDuplicate.code(), "tx_duplicate");
        assert_eq!(RpcError::MempoolFull.code(), "mempool_full");
        assert_eq!(RpcError::NodeBusy.code(), "node_busy");
        assert_eq!(RpcError::NotFound.code(), "not_found");
    }

    #[test]
    fn display_matches_code() {
        assert_eq!(RpcError::TxDuplicate.to_string(), "tx_duplicate");
    }

    #[test]
    fn status_code_matches_the_adrs_own_named_codes() {
        assert_eq!(RpcError::InvalidAddress.status_code(), 400);
        assert_eq!(RpcError::BlockNotFound.status_code(), 404);
        assert_eq!(RpcError::NotFound.status_code(), 404);
        assert_eq!(RpcError::TxDecodeFailed.status_code(), 422);
        assert_eq!(RpcError::TxDuplicate.status_code(), 422);
        assert_eq!(RpcError::MempoolFull.status_code(), 422);
        assert_eq!(RpcError::NodeBusy.status_code(), 503);
    }
}
