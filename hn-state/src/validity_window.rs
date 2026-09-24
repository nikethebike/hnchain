use hn_core::BlockHeight;
use hn_hncs::{Decoder, HncsResult, write_optional, write_u64};

use crate::error::{StateError, StateResult};

/// The `TransactionEnvelope.validity_window` field (ADR-0006, "Validity
/// Window"): bounds how long a transaction may sit unconfirmed before it
/// must be dropped. Height-based, not epoch-based — epochs are too
/// coarse (validator-set/protocol-parameter periods) for this purpose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidityWindowV1 {
    /// Absent means valid from genesis (no lower bound).
    pub min_height: Option<BlockHeight>,
    /// Absent means no expiry (no upper bound).
    pub max_height: Option<BlockHeight>,
}

impl ValidityWindowV1 {
    /// Encodes this value as canonical HNCS bytes (ADR-0006, "Validity
    /// Window").
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::with_capacity(2 + 16);
        self.encode_into(&mut out).map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Appends this value's canonical HNCS bytes to `out`. Shared by
    /// [`ValidityWindowV1::encode`] and by
    /// [`crate::TransactionEnvelope`], which embeds this value flat
    /// rather than as a separately length-prefixed blob — the same
    /// `encode_into`/`decode_from` convention
    /// [`hn_crypto::SignatureEnvelope`] already established. Both
    /// bounds are independent HNCS `optional` fields (ADR-0004), not a
    /// sentinel value, and `ValidityWindowV1` itself is not nested in
    /// an outer `optional`: "no window" is already expressible as both
    /// bounds absent. `HncsResult`-typed: neither bound's decode can
    /// fail with a domain-specific error (every `u64` is a valid
    /// height), so this composes directly with generic HNCS helpers,
    /// unlike [`crate::key_descriptor::decode_key_descriptor`].
    pub fn encode_into(&self, out: &mut Vec<u8>) -> HncsResult<()> {
        write_optional(out, self.min_height.as_ref(), |out, height| {
            write_u64(out, height.get());
            Ok(())
        })?;
        write_optional(out, self.max_height.as_ref(), |out, height| {
            write_u64(out, height.get());
            Ok(())
        })?;
        Ok(())
    }

    /// Decodes canonical HNCS bytes produced by
    /// [`ValidityWindowV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let window = Self::decode_from(&mut decoder).map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;
        Ok(window)
    }

    /// Decodes this value's fields from `decoder` without requiring the
    /// decoder to be exhausted afterward — the counterpart to
    /// [`ValidityWindowV1::encode_into`], shared the same way.
    pub fn decode_from(decoder: &mut Decoder<'_>) -> HncsResult<Self> {
        let min_height =
            decoder.read_optional(|decoder| decoder.read_u64().map(BlockHeight::new))?;
        let max_height =
            decoder.read_optional(|decoder| decoder.read_u64().map(BlockHeight::new))?;

        Ok(Self {
            min_height,
            max_height,
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_core::BlockHeight;

    use super::ValidityWindowV1;

    #[test]
    fn encodes_both_absent_matching_independent_oracle() -> crate::error::StateResult<()> {
        let window = ValidityWindowV1 {
            min_height: None,
            max_height: None,
        };
        assert_eq!(hex(&window.encode()?), "0000");
        Ok(())
    }

    #[test]
    fn encodes_both_present_matching_independent_oracle() -> crate::error::StateResult<()> {
        let window = ValidityWindowV1 {
            min_height: Some(BlockHeight::new(100)),
            max_height: Some(BlockHeight::new(200)),
        };
        assert_eq!(
            hex(&window.encode()?),
            "01640000000000000001c800000000000000"
        );
        Ok(())
    }

    #[test]
    fn encodes_only_max_matching_independent_oracle() -> crate::error::StateResult<()> {
        let window = ValidityWindowV1 {
            min_height: None,
            max_height: Some(BlockHeight::new(5000)),
        };
        assert_eq!(hex(&window.encode()?), "00018813000000000000");
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        for window in [
            ValidityWindowV1 {
                min_height: None,
                max_height: None,
            },
            ValidityWindowV1 {
                min_height: Some(BlockHeight::new(1)),
                max_height: Some(BlockHeight::new(2)),
            },
            ValidityWindowV1 {
                min_height: Some(BlockHeight::new(1)),
                max_height: None,
            },
        ] {
            let decoded = ValidityWindowV1::decode(&window.encode()?)?;
            assert_eq!(decoded, window);
        }
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> crate::error::StateResult<()> {
        let window = ValidityWindowV1 {
            min_height: None,
            max_height: None,
        };
        let mut encoded = window.encode()?;
        encoded.push(0xff);
        assert!(ValidityWindowV1::decode(&encoded).is_err());
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
