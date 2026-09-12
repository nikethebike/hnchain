use hn_crypto::{Digest, hash_profile_0x0001};
use hn_hncs::{Decoder, write_fixed_bytes, write_u8, write_u16};

use crate::error::{StateError, StateResult};

/// `receipt_version` for the current `ReceiptV1` shape (ADR-0006,
/// "Receipts").
pub const RECEIPT_VERSION_1: u16 = 1;

/// The `status` registry (ADR-0006, "Receipts"), closed for this
/// profile. No third status is needed: a transaction failing precheck
/// is never included and produces no receipt at all, so every included
/// transaction is either `Failed` (payload validation precondition
/// failed; nonce and fee still applied) or `Success`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReceiptStatus {
    /// The transaction was included but its payload's own execution
    /// failed; only nonce and fee effects applied.
    Failed = 0x00,
    /// The payload's state transition fully applied.
    Success = 0x01,
}

impl ReceiptStatus {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x00 => Ok(Self::Failed),
            0x01 => Ok(Self::Success),
            _ => Err(StateError::InvalidReceiptStatus { value }),
        }
    }

    const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// A transaction's execution receipt (ADR-0006, "Receipts"): the
/// minimal core shape decided so far. `fee_charged`, `resource_usage`,
/// and `emitted_event_references` are deliberately not fields yet --
/// they are a future `receipt_version` bump once the fee model and an
/// event model exist to inform their shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptV1 {
    /// The transaction this receipt belongs to.
    pub tx_id: Digest,
    /// Whether the transaction's payload succeeded.
    pub status: ReceiptStatus,
}

impl ReceiptV1 {
    /// Encodes this value as canonical HNCS bytes (ADR-0006,
    /// "Receipts").
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + 32 + 1);
        write_u16(&mut out, RECEIPT_VERSION_1);
        write_fixed_bytes(&mut out, &self.tx_id);
        write_u8(&mut out, self.status.as_u8());
        out
    }

    /// Computes this receipt's digest (ADR-0008, "Ordered List
    /// Commitment"): `HASH_PROFILE_0x0001("hnchain.receipt.v1",
    /// HNCS(ReceiptV1))`. This is the leaf value `receipts_root` (ADR-0008)
    /// commits to — not the same as `tx_id` (which identifies the
    /// transaction this receipt is *for*, not the receipt itself).
    pub fn digest(&self) -> StateResult<Digest> {
        Ok(hash_profile_0x0001("hnchain.receipt.v1", &self.encode())?)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ReceiptV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let receipt_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if receipt_version != RECEIPT_VERSION_1 {
            return Err(StateError::UnsupportedReceiptVersion {
                value: receipt_version,
            });
        }

        let tx_id = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let status = ReceiptStatus::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { tx_id, status })
    }
}

#[cfg(test)]
mod tests {
    use super::{ReceiptStatus, ReceiptV1, StateError};

    const TX_ID: [u8; 32] = [0xaa; 32];

    #[test]
    fn encodes_success_matching_independent_oracle() {
        let receipt = ReceiptV1 {
            tx_id: TX_ID,
            status: ReceiptStatus::Success,
        };
        assert_eq!(
            hex(&receipt.encode()),
            "0100aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa01"
        );
    }

    #[test]
    fn digest_matches_independent_oracle() -> crate::error::StateResult<()> {
        let receipt = ReceiptV1 {
            tx_id: TX_ID,
            status: ReceiptStatus::Success,
        };
        assert_eq!(
            hex(&receipt.digest()?),
            "c3aa176ddcff4b7b5072f87e8b64922972f9bb5189163f3cf22b0a7bbfc713c9"
        );
        Ok(())
    }

    #[test]
    fn encodes_failed_matching_independent_oracle() {
        let receipt = ReceiptV1 {
            tx_id: TX_ID,
            status: ReceiptStatus::Failed,
        };
        assert_eq!(
            hex(&receipt.encode()),
            "0100aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa00"
        );
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        for status in [ReceiptStatus::Failed, ReceiptStatus::Success] {
            let receipt = ReceiptV1 {
                tx_id: TX_ID,
                status,
            };
            let decoded = ReceiptV1::decode(&receipt.encode())?;
            assert_eq!(decoded, receipt);
        }
        Ok(())
    }

    #[test]
    fn rejects_unsupported_receipt_version() {
        let receipt = ReceiptV1 {
            tx_id: TX_ID,
            status: ReceiptStatus::Success,
        };
        let mut encoded = receipt.encode();
        encoded[0] = 0x02; // receipt_version low byte, little-endian
        assert_eq!(
            ReceiptV1::decode(&encoded),
            Err(StateError::UnsupportedReceiptVersion { value: 2 })
        );
    }

    #[test]
    fn rejects_invalid_status() {
        let receipt = ReceiptV1 {
            tx_id: TX_ID,
            status: ReceiptStatus::Success,
        };
        let mut encoded = receipt.encode();
        let last = encoded.len() - 1;
        encoded[last] = 0x02; // one past the closed registry's last value
        assert_eq!(
            ReceiptV1::decode(&encoded),
            Err(StateError::InvalidReceiptStatus { value: 0x02 })
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let receipt = ReceiptV1 {
            tx_id: TX_ID,
            status: ReceiptStatus::Success,
        };
        let mut encoded = receipt.encode();
        encoded.push(0xff);
        assert!(ReceiptV1::decode(&encoded).is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
