use hn_crypto::Digest;
use hn_hncs::{Decoder, write_fixed_bytes, write_optional, write_u16, write_u128};

use crate::error::{StateError, StateResult};

/// `payload_version` for the current `TransferPayloadV1` shape
/// (ADR-0006, "Payload", `transfer`).
pub const TRANSFER_PAYLOAD_VERSION_1: u16 = 1;

/// The `transfer` (`tx_type = 0x01`) payload (ADR-0006, "Payload"): moves
/// `amount` from the transaction's `sender` to `recipient`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferPayloadV1 {
    /// The recipient's `address_body` — same shape as `sender`
    /// (ADR-0006, "Sender").
    pub recipient: Digest,
    /// Absent means native HNCOIN (Balance State, account-state.md
    /// §4.3); present references a curated protocol-level/bridged asset
    /// (Asset State, §4.7). Contract-defined assets are out of scope —
    /// those move through `contract_call`, not `transfer`.
    pub asset_id: Option<u16>,
    /// The amount to move. May be zero (a valid no-op, not a validation
    /// error).
    pub amount: u128,
}

impl TransferPayloadV1 {
    /// Encodes this value as canonical HNCS bytes (ADR-0006, "Payload").
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::with_capacity(2 + 32 + 3 + 16);
        write_u16(&mut out, TRANSFER_PAYLOAD_VERSION_1);
        write_fixed_bytes(&mut out, &self.recipient);
        write_optional(&mut out, self.asset_id.as_ref(), |out, asset_id| {
            write_u16(out, *asset_id);
            Ok(())
        })
        .map_err(StateError::Encoding)?;
        write_u128(&mut out, self.amount);
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`TransferPayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let payload_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if payload_version != TRANSFER_PAYLOAD_VERSION_1 {
            return Err(StateError::UnsupportedTransferPayloadVersion {
                value: payload_version,
            });
        }

        let recipient = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let asset_id = decoder
            .read_optional(Decoder::read_u16)
            .map_err(StateError::Encoding)?;
        let amount = decoder.read_u128().map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            recipient,
            asset_id,
            amount,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{StateError, TransferPayloadV1};

    const RECIPIENT: [u8; 32] = [0x11; 32];

    #[test]
    fn encodes_native_transfer_matching_independent_oracle() -> crate::error::StateResult<()> {
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT,
            asset_id: None,
            amount: 5000,
        };
        assert_eq!(
            hex(&payload.encode()?),
            "0100\
             1111111111111111111111111111111111111111111111111111111111111111\
             00\
             88130000000000000000000000000000"
        );
        Ok(())
    }

    #[test]
    fn encodes_asset_transfer_matching_independent_oracle() -> crate::error::StateResult<()> {
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT,
            asset_id: Some(7),
            amount: 250,
        };
        assert_eq!(
            hex(&payload.encode()?),
            "0100\
             1111111111111111111111111111111111111111111111111111111111111111\
             010700\
             fa000000000000000000000000000000"
        );
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        for payload in [
            TransferPayloadV1 {
                recipient: RECIPIENT,
                asset_id: None,
                amount: 5000,
            },
            TransferPayloadV1 {
                recipient: RECIPIENT,
                asset_id: Some(7),
                amount: 250,
            },
        ] {
            let decoded = TransferPayloadV1::decode(&payload.encode()?)?;
            assert_eq!(decoded, payload);
        }
        Ok(())
    }

    #[test]
    fn rejects_unsupported_payload_version() -> crate::error::StateResult<()> {
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT,
            asset_id: None,
            amount: 1,
        };
        let mut encoded = payload.encode()?;
        encoded[0] = 0x02; // payload_version low byte, little-endian
        assert_eq!(
            TransferPayloadV1::decode(&encoded),
            Err(StateError::UnsupportedTransferPayloadVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> crate::error::StateResult<()> {
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT,
            asset_id: None,
            amount: 1,
        };
        let mut encoded = payload.encode()?;
        encoded.push(0xff);
        assert!(TransferPayloadV1::decode(&encoded).is_err());
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
