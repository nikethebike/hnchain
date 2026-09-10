//! End-to-end proof that `hn-list-merkle-v1` composes with real protocol
//! data, not synthetic placeholder bytes: derive real account addresses,
//! build three real `TransferPayloadV1`s between them, compute each
//! one's `tx_id` and a `ReceiptV1` digest for it, and commit both
//! ordered lists via `list_merkle_root` — the same path
//! `transactions_root`/`receipts_root` (ADR-0008) would take.
//!
//! `TransactionEnvelope` has no concrete Rust type yet (several of its
//! fields are still open), so `tx_id` is computed here over each
//! transfer's own canonical `TransferPayloadV1` bytes rather than a full
//! envelope encoding — an approximation of the real input, not a claim
//! that this *is* a complete envelope. Every other value (addresses,
//! payload bytes, tx_id, receipt digests, both roots) is real,
//! oracle-verified protocol data.

use hn_crypto::account_address_body;
use hn_state::{
    ReceiptStatus, ReceiptV1, TransferPayloadV1, list_merkle_root, tx_id as compute_tx_id,
};

const RECIPIENT_PUBLIC_KEY: [u8; 32] = [0xab; 32];
const RECIPIENT2_PUBLIC_KEY: [u8; 32] = [0xcd; 32];

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn transactions_and_receipts_roots_match_independent_oracle() -> TestResult {
    let recipient_address = account_address_body(0x0001, 0x0001, &RECIPIENT_PUBLIC_KEY)?;
    let recipient2_address = account_address_body(0x0001, 0x0001, &RECIPIENT2_PUBLIC_KEY)?;
    assert_eq!(
        hex(&recipient_address),
        "a7f0880ea0c5f0e910063e76c8e02a54d4f9ff47dac7ea3e127b728d80cf0ddc"
    );
    assert_eq!(
        hex(&recipient2_address),
        "73605c3a801828b56eedcd67fdd279dfd8486413318b3289f97f40c1b8559a4e"
    );

    // Three real transfers: two native, one asset-denominated, to two
    // distinct recipients.
    let payloads = [
        TransferPayloadV1 {
            recipient: recipient_address,
            asset_id: None,
            amount: 100,
        },
        TransferPayloadV1 {
            recipient: recipient2_address,
            asset_id: None,
            amount: 250,
        },
        TransferPayloadV1 {
            recipient: recipient_address,
            asset_id: Some(7),
            amount: 50,
        },
    ];

    let mut tx_ids = Vec::new();
    for payload in &payloads {
        tx_ids.push(compute_tx_id(&payload.encode()?)?);
    }

    let expected_tx_ids = [
        "51df7f5d043398144b5230c6e2c5161d496dedaa8ee8675e3631f117cf095f8a",
        "c4d8e53e6015ce9103a557a09d885dd552efe64425f303194e4fa3ef0b6d5956",
        "a6f82ab5b5477703616379871b18ebdaff05f9f4c5b707a6c0466247f39ec6df",
    ];
    for (id, expected) in tx_ids.iter().zip(expected_tx_ids) {
        assert_eq!(hex(id), expected);
    }

    let transactions_root = list_merkle_root(&tx_ids)?;
    assert_eq!(
        hex(&transactions_root),
        "aabb06cb4edacaf36afd998788cbc06e3fdeca112c6c37d28308672667ec1bb2"
    );

    // Two succeed, the third (the asset transfer) fails -- exercising
    // both ReceiptStatus values with real tx_id linkage, not just
    // uniform success.
    let statuses = [
        ReceiptStatus::Success,
        ReceiptStatus::Success,
        ReceiptStatus::Failed,
    ];
    let mut receipt_digests = Vec::new();
    for (id, status) in tx_ids.iter().zip(statuses) {
        let receipt = ReceiptV1 { tx_id: *id, status };
        receipt_digests.push(receipt.digest()?);
    }

    let expected_receipt_digests = [
        "14c169f4322e1b41ade22e35e76646632933831d2528126060fbeddb8555f68a",
        "44337f9b11fb651c971ef543a0e917e99e02900c6f766853d88eab0419422981",
        "12befcaeb0caf6af29d445541e18aa85db082aea23d73e7a761a096f75b7e980",
    ];
    for (digest, expected) in receipt_digests.iter().zip(expected_receipt_digests) {
        assert_eq!(hex(digest), expected);
    }

    let receipts_root = list_merkle_root(&receipt_digests)?;
    assert_eq!(
        hex(&receipts_root),
        "048032696080e8b77eb91e4627b5283e1bc2f2addd03cc92421364144a2fa020"
    );

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
