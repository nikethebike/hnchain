use hn_hncs::{Decoder, write_u16, write_u128};

use crate::error::{StateError, StateResult};

/// `payload_version` for the current `UnstakePayloadV1` shape (ADR-0006,
/// "Decided: `stake`/`unstake`/`validator_update` payload shapes").
pub const UNSTAKE_PAYLOAD_VERSION_1: u16 = 1;

/// `unstake` (`tx_type = 0x05`) payload: decreases the sender's bonded
/// stake by `amount`. Same shape and implicit-`sender`-as-`validator_id`
/// reasoning as [`crate::StakePayloadV1`] — see its own documentation.
///
/// State transition (not implemented by this type): `bonded_stake -=
/// amount`, checked — cannot go negative. Deliberately decides only
/// this immediate bookkeeping effect, not whether or when unstaked
/// funds actually become withdrawable — that needs the still-open
/// "unbonding period" (ADR-0010) and is a real mechanism this decision
/// does not resolve, not merely an unfilled constant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnstakePayloadV1 {
    /// Amount subtracted from `bonded_stake`.
    pub amount: u128,
}

impl UnstakePayloadV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, UNSTAKE_PAYLOAD_VERSION_1);
        write_u128(&mut out, self.amount);
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`UnstakePayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let payload_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if payload_version != UNSTAKE_PAYLOAD_VERSION_1 {
            return Err(StateError::UnsupportedUnstakePayloadVersion {
                value: payload_version,
            });
        }

        let amount = decoder.read_u128().map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { amount })
    }
}

#[cfg(test)]
mod tests {
    use super::{StateError, UnstakePayloadV1};
    use crate::error::StateResult;

    fn sample() -> UnstakePayloadV1 {
        UnstakePayloadV1 {
            amount: 0x0123_4567_89ab_cdef_0011_2233_4455_6677,
        }
    }

    #[test]
    fn encodes_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&sample().encode()?),
            "01007766554433221100efcdab8967452301"
        );
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let payload = sample();
        let decoded = UnstakePayloadV1::decode(&payload.encode()?)?;
        assert_eq!(decoded, payload);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_payload_version() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded[0] = 0x02; // payload_version low byte, little-endian
        assert_eq!(
            UnstakePayloadV1::decode(&encoded),
            Err(StateError::UnsupportedUnstakePayloadVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded.push(0x00);
        assert!(matches!(
            UnstakePayloadV1::decode(&encoded),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
