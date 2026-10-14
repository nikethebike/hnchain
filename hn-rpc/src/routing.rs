use hn_core::BlockHeight;

use crate::call::RpcCall;
use crate::error::RpcError;
use crate::hex::{decode as hex_decode, decode_32 as hex_decode_32};
use crate::parser::{HttpMethod, ParsedRequest};

/// Matches `request` against ADR-0046's own "Decided: v1 Endpoint
/// Set" — exactly eight `(method, path)` shapes; anything else,
/// including a recognized path with the wrong method, is
/// [`RpcError::NotFound`]. Path parameters (`height`, `address`,
/// `tx_id`) are decoded here (decimal for height, hex for the
/// 32-byte ids) — a value that does not even look like the right
/// shape for its own position is the same "this is not a route this
/// surface defines" case as an unmatched path entirely, not a
/// separate error code.
pub fn route(request: &ParsedRequest<'_>) -> Result<RpcCall, RpcError> {
    let segments: Vec<&str> = request
        .target
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    match (request.method, segments.as_slice()) {
        (HttpMethod::Get, ["v1", "status"]) => Ok(RpcCall::Status),
        (HttpMethod::Get, ["v1", "blocks", "latest"]) => Ok(RpcCall::LatestBlock),
        (HttpMethod::Get, ["v1", "blocks", height]) => {
            let height: u64 = height.parse().map_err(|_| RpcError::NotFound)?;
            Ok(RpcCall::BlockAtHeight {
                height: BlockHeight::new(height),
            })
        }
        (HttpMethod::Get, ["v1", "accounts", address]) => {
            let address = hex_decode_32(address).ok_or(RpcError::InvalidAddress)?;
            Ok(RpcCall::Account { address })
        }
        (HttpMethod::Get, ["v1", "transactions", tx_id]) => {
            let tx_id = hex_decode_32(tx_id).ok_or(RpcError::NotFound)?;
            Ok(RpcCall::Transaction { tx_id })
        }
        (HttpMethod::Post, ["v1", "transactions"]) => {
            let tx_bytes = decode_submit_body(request.body)?;
            Ok(RpcCall::SubmitTransaction { tx_bytes })
        }
        (HttpMethod::Get, ["v1", "mempool"]) => Ok(RpcCall::Mempool),
        (HttpMethod::Get, ["v1", "validators"]) => Ok(RpcCall::Validators),
        _ => Err(RpcError::NotFound),
    }
}

/// Decodes `POST /v1/transactions`'s own body (ADR-0046, "Decided:
/// Transaction Submission" — `{"tx": "<hex of canonical
/// TransactionEnvelope>"}`). Malformed JSON, a missing/non-string
/// `tx` field, and non-hex content all fold into
/// [`RpcError::TxDecodeFailed`] — every one of them means the
/// submission itself could not even be read, the same umbrella
/// `Mempool::admit`'s own step 1 already uses for unreadable raw
/// bytes.
fn decode_submit_body(body: &[u8]) -> Result<Vec<u8>, RpcError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| RpcError::TxDecodeFailed)?;
    let tx_hex = value
        .get("tx")
        .and_then(serde_json::Value::as_str)
        .ok_or(RpcError::TxDecodeFailed)?;
    hex_decode(tx_hex).ok_or(RpcError::TxDecodeFailed)
}

#[cfg(test)]
mod tests {
    use hn_core::BlockHeight;

    use super::route;
    use crate::call::RpcCall;
    use crate::error::RpcError;
    use crate::hex::encode as hex_encode;
    use crate::parser::{HttpMethod, ParsedRequest};

    fn request<'a>(method: HttpMethod, target: &'a str, body: &'a [u8]) -> ParsedRequest<'a> {
        ParsedRequest {
            method,
            target,
            headers: Vec::new(),
            body,
        }
    }

    #[test]
    fn routes_status() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/status", b"")),
            Ok(RpcCall::Status)
        );
    }

    #[test]
    fn routes_latest_block() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/blocks/latest", b"")),
            Ok(RpcCall::LatestBlock)
        );
    }

    #[test]
    fn routes_block_at_height() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/blocks/42", b"")),
            Ok(RpcCall::BlockAtHeight {
                height: BlockHeight::new(42)
            })
        );
    }

    #[test]
    fn rejects_a_non_numeric_height() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/blocks/not-a-number", b"")),
            Err(RpcError::NotFound)
        );
    }

    #[test]
    fn routes_an_account_lookup() {
        let address = [0x11_u8; 32];
        let target = format!("/v1/accounts/{}", hex_encode(&address));
        assert_eq!(
            route(&request(HttpMethod::Get, &target, b"")),
            Ok(RpcCall::Account { address })
        );
    }

    #[test]
    fn rejects_a_malformed_account_address() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/accounts/not-hex", b"")),
            Err(RpcError::InvalidAddress)
        );
    }

    #[test]
    fn routes_a_transaction_lookup() {
        let tx_id = [0x22_u8; 32];
        let target = format!("/v1/transactions/{}", hex_encode(&tx_id));
        assert_eq!(
            route(&request(HttpMethod::Get, &target, b"")),
            Ok(RpcCall::Transaction { tx_id })
        );
    }

    #[test]
    fn rejects_a_malformed_transaction_id() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/transactions/not-hex", b"")),
            Err(RpcError::NotFound)
        );
    }

    #[test]
    fn routes_a_transaction_submission() {
        let body = br#"{"tx": "deadbeef"}"#;
        assert_eq!(
            route(&request(HttpMethod::Post, "/v1/transactions", body)),
            Ok(RpcCall::SubmitTransaction {
                tx_bytes: vec![0xde, 0xad, 0xbe, 0xef]
            })
        );
    }

    #[test]
    fn rejects_a_submission_with_malformed_json() {
        assert_eq!(
            route(&request(HttpMethod::Post, "/v1/transactions", b"not json")),
            Err(RpcError::TxDecodeFailed)
        );
    }

    #[test]
    fn rejects_a_submission_missing_the_tx_field() {
        assert_eq!(
            route(&request(HttpMethod::Post, "/v1/transactions", b"{}")),
            Err(RpcError::TxDecodeFailed)
        );
    }

    #[test]
    fn rejects_a_submission_with_non_hex_tx() {
        let body = br#"{"tx": "not-hex"}"#;
        assert_eq!(
            route(&request(HttpMethod::Post, "/v1/transactions", body)),
            Err(RpcError::TxDecodeFailed)
        );
    }

    #[test]
    fn routes_mempool() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/mempool", b"")),
            Ok(RpcCall::Mempool)
        );
    }

    #[test]
    fn routes_validators() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/validators", b"")),
            Ok(RpcCall::Validators)
        );
    }

    #[test]
    fn rejects_an_unknown_path() {
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/unknown", b"")),
            Err(RpcError::NotFound)
        );
    }

    #[test]
    fn rejects_a_known_path_with_the_wrong_method() {
        assert_eq!(
            route(&request(HttpMethod::Post, "/v1/status", b"")),
            Err(RpcError::NotFound)
        );
        assert_eq!(
            route(&request(HttpMethod::Get, "/v1/transactions", b"")),
            Err(RpcError::NotFound)
        );
    }
}
