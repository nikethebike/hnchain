use serde_json::{Value, json};

use crate::hex::encode as hex_encode;
use crate::reply::{
    AccountReply, BlockHeaderReply, BlockReply, CommitQcSummary, MempoolReply, ReceiptEntry,
    RpcReply, StatusReply, SubmittedReply, TransactionReply, TransactionStatus, ValidatorEntry,
    ValidatorStatusReply, ValidatorsReply,
};
use crate::response::HttpResponse;

/// Encodes `reply` as this surface's own JSON response body
/// (ADR-0046, "Decided: Value Encoding Rules"): byte strings as
/// lowercase hex, amounts/u128 quantities as decimal strings, every
/// other number as a JSON number, never a float. Field names mirror
/// each reply type's own Rust field names directly.
#[must_use]
pub fn reply_to_response(reply: &RpcReply) -> HttpResponse {
    let (status, body) = match reply {
        RpcReply::Status(status_reply) => (200, status_json(status_reply)),
        RpcReply::Block(block_reply) => (200, block_json(block_reply)),
        RpcReply::Account(account_reply) => (200, account_json(account_reply)),
        RpcReply::Transaction(transaction_reply) => (200, transaction_json(transaction_reply)),
        // ADR-0046, "Decided: Transaction Submission": "On success:
        // 202 {...}".
        RpcReply::Submitted(submitted_reply) => (202, submitted_json(submitted_reply)),
        RpcReply::Mempool(mempool_reply) => (200, mempool_json(mempool_reply)),
        RpcReply::Validators(validators_reply) => (200, validators_json(validators_reply)),
    };
    HttpResponse {
        status,
        content_type: "application/json",
        body: body.to_string().into_bytes(),
    }
}

/// The unified error body every rejection on this surface shares
/// (ADR-0046, "Decided: v1 Endpoint Set" — "a unified error format"):
/// `{"error": "<stable code>"}`, nothing else — no stack trace, no
/// internal path (ADR-0046, "Security Considerations").
#[must_use]
pub fn error_response(status: u16, code: &str) -> HttpResponse {
    HttpResponse {
        status,
        content_type: "application/json",
        body: json!({ "error": code }).to_string().into_bytes(),
    }
}

fn status_json(reply: &StatusReply) -> Value {
    json!({
        "chain_id": reply.chain_id,
        "network_id": reply.network_id,
        "genesis_hash": hex_encode(&reply.genesis_hash),
        "protocol_version": format!(
            "{}.{}.{}",
            reply.protocol_version.0, reply.protocol_version.1, reply.protocol_version.2
        ),
        "latest_height": reply.latest_height,
        "latest_block_hash": hex_encode(&reply.latest_block_hash),
        "latest_timestamp": reply.latest_timestamp,
        "syncing": reply.syncing,
        "peer_count": reply.peer_count,
        "mempool_size": reply.mempool_size,
    })
}

fn block_header_json(header: &BlockHeaderReply) -> Value {
    json!({
        "header_version": header.header_version,
        "chain_id": header.chain_id,
        "network_id": header.network_id,
        "height": header.height,
        "round": header.round,
        "epoch": header.epoch,
        "protocol_epoch": header.protocol_epoch,
        "parent_block_hash": hex_encode(&header.parent_block_hash),
        "proposer": hex_encode(&header.proposer),
        "timestamp": header.timestamp,
        "transactions_root": hex_encode(&header.transactions_root),
        "state_root": hex_encode(&header.state_root),
        "receipts_root": hex_encode(&header.receipts_root),
        "events_root": hex_encode(&header.events_root),
        "consensus_root": hex_encode(&header.consensus_root),
        "evidence_root": hex_encode(&header.evidence_root),
        "protocol_parameters_hash": hex_encode(&header.protocol_parameters_hash),
        "extra_data_hash": hex_encode(&header.extra_data_hash),
    })
}

fn receipt_entry_json(entry: &ReceiptEntry) -> Value {
    json!({
        "tx_id": hex_encode(&entry.tx_id),
        "status": if entry.success { "success" } else { "failed" },
    })
}

fn commit_qc_summary_json(summary: &CommitQcSummary) -> Value {
    json!({
        "height": summary.height,
        "round": summary.round,
        "signer_count": summary.signer_count,
    })
}

fn block_json(reply: &BlockReply) -> Value {
    json!({
        "header": block_header_json(&reply.header),
        "tx_ids": reply.tx_ids.iter().map(|tx_id| hex_encode(tx_id)).collect::<Vec<_>>(),
        "receipts": reply.receipts.iter().map(receipt_entry_json).collect::<Vec<_>>(),
        "commit_qc": commit_qc_summary_json(&reply.commit_qc),
    })
}

fn account_json(reply: &AccountReply) -> Value {
    json!({
        "exists": reply.exists,
        "balance": reply.balance.to_string(),
        "nonce": reply.nonce,
        "height": reply.height,
    })
}

fn transaction_json(reply: &TransactionReply) -> Value {
    let mut value = json!({ "tx_id": hex_encode(&reply.tx_id) });
    let Value::Object(map) = &mut value else {
        return value;
    };
    match reply.status {
        TransactionStatus::Finalized {
            height,
            index,
            success,
        } => {
            map.insert("status".to_string(), json!("finalized"));
            map.insert("height".to_string(), json!(height));
            map.insert("index".to_string(), json!(index));
            map.insert(
                "receipt".to_string(),
                json!({ "status": if success { "success" } else { "failed" } }),
            );
        }
        TransactionStatus::Pending => {
            map.insert("status".to_string(), json!("pending"));
        }
        TransactionStatus::Unknown => {
            map.insert("status".to_string(), json!("unknown"));
        }
    }
    value
}

fn submitted_json(reply: &SubmittedReply) -> Value {
    json!({
        "tx_id": hex_encode(&reply.tx_id),
        "status": "accepted",
    })
}

fn mempool_json(reply: &MempoolReply) -> Value {
    json!({
        "size": reply.size,
        "capacity": reply.capacity,
    })
}

fn validator_status_json(status: ValidatorStatusReply) -> &'static str {
    match status {
        ValidatorStatusReply::Registered => "registered",
        ValidatorStatusReply::Candidate => "candidate",
        ValidatorStatusReply::Active => "active",
        ValidatorStatusReply::Inactive => "inactive",
        ValidatorStatusReply::Jailed => "jailed",
        ValidatorStatusReply::Exited => "exited",
    }
}

fn validator_entry_json(entry: &ValidatorEntry) -> Value {
    json!({
        "address": hex_encode(&entry.address),
        "status": validator_status_json(entry.status),
        "bonded_stake": entry.bonded_stake.to_string(),
    })
}

fn validators_json(reply: &ValidatorsReply) -> Value {
    json!({
        "validators": reply.validators.iter().map(validator_entry_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::{error_response, reply_to_response};
    use crate::error::RpcError;
    use crate::parser::HttpError;
    use crate::reply::{
        AccountReply, BlockHeaderReply, BlockReply, CommitQcSummary, MempoolReply, ReceiptEntry,
        RpcReply, StatusReply, SubmittedReply, TransactionReply, TransactionStatus, ValidatorEntry,
        ValidatorStatusReply, ValidatorsReply,
    };

    /// Every response body this module ever produces is UTF-8 JSON by
    /// construction (`serde_json::Value::to_string`); `unwrap_or_default`
    /// rather than `expect`/`unwrap` only because this workspace's own
    /// lints deny both outright, not because this could realistically
    /// fail.
    fn body_text(body: Vec<u8>) -> String {
        String::from_utf8(body).unwrap_or_default()
    }

    // -- Golden response-schema tests (ADR-0046's own conformance
    // requirement: "every endpoint's response schema (golden JSON)").
    // Each asserts the exact byte-for-byte JSON body, not merely that
    // it parses or contains the right fields.

    #[test]
    fn status_golden_schema() {
        let reply = RpcReply::Status(StatusReply {
            chain_id: 1,
            network_id: 1,
            genesis_hash: [0x11; 32],
            protocol_version: (0, 1, 0),
            latest_height: 5,
            latest_block_hash: [0x22; 32],
            latest_timestamp: 1_700_000_000_000,
            syncing: false,
            peer_count: 3,
            mempool_size: 2,
        });
        let response = reply_to_response(&reply);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(
            body_text(response.body),
            concat!(
                r#"{"chain_id":1,"genesis_hash":"1111111111111111111111111111111111111111111111111111111111111111","latest_block_hash":"2222222222222222222222222222222222222222222222222222222222222222","#,
                r#""latest_height":5,"latest_timestamp":1700000000000,"mempool_size":2,"network_id":1,"peer_count":3,"protocol_version":"0.1.0","syncing":false}"#
            )
        );
    }

    #[test]
    fn block_golden_schema() {
        let reply = RpcReply::Block(Box::new(BlockReply {
            header: BlockHeaderReply {
                header_version: 1,
                chain_id: 1,
                network_id: 1,
                height: 7,
                round: 0,
                epoch: 0,
                protocol_epoch: 0,
                parent_block_hash: [0x01; 32],
                proposer: [0x02; 32],
                timestamp: 1_700_000_000_000,
                transactions_root: [0x03; 32],
                state_root: [0x04; 32],
                receipts_root: [0x05; 32],
                events_root: [0x06; 32],
                consensus_root: [0x07; 32],
                evidence_root: [0x08; 32],
                protocol_parameters_hash: [0x09; 32],
                extra_data_hash: [0x0a; 32],
            },
            tx_ids: vec![[0x0b; 32]],
            receipts: vec![ReceiptEntry {
                tx_id: [0x0b; 32],
                success: true,
            }],
            commit_qc: CommitQcSummary {
                height: 7,
                round: 0,
                signer_count: 3,
            },
        }));
        let response = reply_to_response(&reply);
        assert_eq!(response.status, 200);
        let body = body_text(response.body);
        assert_eq!(
            body,
            concat!(
                r#"{"commit_qc":{"height":7,"round":0,"signer_count":3},"#,
                r#""header":{"chain_id":1,"consensus_root":"0707070707070707070707070707070707070707070707070707070707070707","#,
                r#""epoch":0,"events_root":"0606060606060606060606060606060606060606060606060606060606060606","#,
                r#""evidence_root":"0808080808080808080808080808080808080808080808080808080808080808","#,
                r#""extra_data_hash":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","#,
                r#""header_version":1,"height":7,"network_id":1,"parent_block_hash":"0101010101010101010101010101010101010101010101010101010101010101","#,
                r#""proposer":"0202020202020202020202020202020202020202020202020202020202020202","protocol_epoch":0,"#,
                r#""protocol_parameters_hash":"0909090909090909090909090909090909090909090909090909090909090909","receipts_root":"0505050505050505050505050505050505050505050505050505050505050505","#,
                r#""round":0,"state_root":"0404040404040404040404040404040404040404040404040404040404040404","#,
                r#""timestamp":1700000000000,"transactions_root":"0303030303030303030303030303030303030303030303030303030303030303"},"#,
                r#""receipts":[{"status":"success","tx_id":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"}],"#,
                r#""tx_ids":["0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"]}"#
            )
        );
    }

    #[test]
    fn account_golden_schema() {
        let reply = RpcReply::Account(AccountReply {
            exists: true,
            balance: 1_000_000_000_000_000_000_000,
            nonce: 3,
            height: 10,
        });
        let response = reply_to_response(&reply);
        assert_eq!(response.status, 200);
        assert_eq!(
            body_text(response.body),
            r#"{"balance":"1000000000000000000000","exists":true,"height":10,"nonce":3}"#
        );
    }

    #[test]
    fn transaction_finalized_golden_schema() {
        let reply = RpcReply::Transaction(TransactionReply {
            tx_id: [0xaa; 32],
            status: TransactionStatus::Finalized {
                height: 4,
                index: 1,
                success: true,
            },
        });
        let response = reply_to_response(&reply);
        assert_eq!(
            body_text(response.body),
            concat!(
                r#"{"height":4,"index":1,"receipt":{"status":"success"},"status":"finalized","#,
                r#""tx_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#
            )
        );
    }

    #[test]
    fn transaction_pending_golden_schema() {
        let reply = RpcReply::Transaction(TransactionReply {
            tx_id: [0xbb; 32],
            status: TransactionStatus::Pending,
        });
        let response = reply_to_response(&reply);
        assert_eq!(
            body_text(response.body),
            r#"{"status":"pending","tx_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#
        );
    }

    #[test]
    fn transaction_unknown_golden_schema() {
        let reply = RpcReply::Transaction(TransactionReply {
            tx_id: [0xcc; 32],
            status: TransactionStatus::Unknown,
        });
        let response = reply_to_response(&reply);
        assert_eq!(
            body_text(response.body),
            r#"{"status":"unknown","tx_id":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}"#
        );
    }

    #[test]
    fn submitted_golden_schema() {
        let reply = RpcReply::Submitted(SubmittedReply { tx_id: [0xdd; 32] });
        let response = reply_to_response(&reply);
        // ADR-0046, "Decided: Transaction Submission": "On success: 202 {...}".
        assert_eq!(response.status, 202);
        assert_eq!(
            body_text(response.body),
            r#"{"status":"accepted","tx_id":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}"#
        );
    }

    #[test]
    fn mempool_golden_schema() {
        let reply = RpcReply::Mempool(MempoolReply {
            size: 4,
            capacity: 10_000,
        });
        let response = reply_to_response(&reply);
        assert_eq!(body_text(response.body), r#"{"capacity":10000,"size":4}"#);
    }

    #[test]
    fn validators_golden_schema() {
        let reply = RpcReply::Validators(ValidatorsReply {
            validators: vec![ValidatorEntry {
                address: [0xee; 32],
                status: ValidatorStatusReply::Active,
                bonded_stake: 500_000_000_000,
            }],
        });
        let response = reply_to_response(&reply);
        assert_eq!(
            body_text(response.body),
            concat!(
                r#"{"validators":[{"address":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","#,
                r#""bonded_stake":"500000000000","status":"active"}]}"#
            )
        );
    }

    // -- Golden error-code tests (ADR-0046's own conformance
    // requirement: "each error code").

    #[test]
    fn rpc_error_golden_bodies() {
        for (error, expected_status) in [
            (RpcError::InvalidAddress, 400),
            (RpcError::BlockNotFound, 404),
            (RpcError::NotFound, 404),
            (RpcError::TxDecodeFailed, 422),
            (RpcError::TxChainMismatch, 422),
            (RpcError::TxBadSignature, 422),
            (RpcError::TxOutsideValidityWindow, 422),
            (RpcError::TxWouldFail, 422),
            (RpcError::TxDuplicate, 422),
            (RpcError::MempoolFull, 422),
            (RpcError::NodeBusy, 503),
        ] {
            let response = error_response(error.status_code(), error.code());
            assert_eq!(response.status, expected_status);
            assert_eq!(response.content_type, "application/json");
            assert_eq!(
                body_text(response.body),
                format!(r#"{{"error":"{}"}}"#, error.code())
            );
        }
    }

    #[test]
    fn http_error_golden_bodies() {
        for (error, expected_status) in [
            (HttpError::MethodNotAllowed, 405),
            (HttpError::ChunkedTransferEncodingNotSupported, 501),
            (HttpError::ExpectNotSupported, 417),
            (HttpError::TruncatedRequest, 400),
            (HttpError::InvalidContentLength, 400),
        ] {
            let response = crate::response::HttpResponse::from_http_error(error);
            assert_eq!(response.status, expected_status);
            assert_eq!(response.content_type, "application/json");
            assert_eq!(
                body_text(response.body),
                format!(r#"{{"error":"{}"}}"#, error.code())
            );
        }
    }
}
