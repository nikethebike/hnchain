use hn_core::BlockHeight;
use hn_hncs::{Decoder, write_optional, write_u64};

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
    /// Window"). Both bounds are independent HNCS `optional` fields
    /// (ADR-0004), not a sentinel value, and `ValidityWindowV1` itself
    /// is not nested in an outer `optional`: "no window" is already
    /// expressible as both bounds absent.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::with_capacity(2 + 16);
        write_optional(&mut out, self.min_height.as_ref(), |out, height| {
            write_u64(out, height.get());
            Ok(())
        })
        .map_err(StateError::Encoding)?;
        write_optional(&mut out, self.max_height.as_ref(), |out, height| {
            write_u64(out, height.get());
            Ok(())
        })
        .map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes canonical HNCS bytes produced by
    /// [`ValidityWindowV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let min_height = decoder
            .read_optional(|decoder| decoder.read_u64().map(BlockHeight::new))
            .map_err(StateError::Encoding)?;
        let max_height = decoder
            .read_optional(|decoder| decoder.read_u64().map(BlockHeight::new))
            .map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;

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
