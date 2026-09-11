use hn_hncs::{Decoder, write_u16, write_u128};

use crate::error::{StateError, StateResult};

/// `payload_version` for the current `StakePayloadV1` shape (ADR-0006,
/// "Decided: `stake`/`unstake`/`validator_update` payload shapes").
pub const STAKE_PAYLOAD_VERSION_1: u16 = 1;

/// `stake` (`tx_type = 0x04`) payload: increases the sender's bonded
/// stake by `amount`.
///
/// No `validator_id` field: `validator_id` is the controlling account's
/// own `address_body` (ADR-0010, "Decided: `validator_id` derivation"),
/// so it is always `sender` — a self-managed validator names itself
/// implicitly, the same way `transfer` never names its own sender.
///
/// Requires a `ValidatorRecordV1` to already exist for `sender`
/// (created via `validator_update { operation: register }`) — unlike
/// `transfer`'s implicit account creation, `stake` does not implicitly
/// create one, because a validator record additionally needs a
/// `consensus_key`, which this payload carries no field for.
///
/// State transition (not implemented by this type — decoding a payload
/// is this crate's concern the same way `TransferPayloadV1` separates
/// codec from `apply_transfer`): `bonded_stake += amount`. Whether
/// `voting_power` is recomputed synchronously or only at the next epoch
/// boundary (ADR-0010, "Decided: `bonded_stake`, distinct from
/// `voting_power`") is not decided. Minimum bond amount stays an open
/// economic parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StakePayloadV1 {
    /// Amount added to `bonded_stake`.
    pub amount: u128,
}

impl StakePayloadV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, STAKE_PAYLOAD_VERSION_1);
        write_u128(&mut out, self.amount);
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`StakePayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let payload_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if payload_version != STAKE_PAYLOAD_VERSION_1 {
            return Err(StateError::UnsupportedStakePayloadVersion {
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
    use super::{StakePayloadV1, StateError};
    use crate::error::StateResult;

    fn sample() -> StakePayloadV1 {
        StakePayloadV1 {
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
        let decoded = StakePayloadV1::decode(&payload.encode()?)?;
        assert_eq!(decoded, payload);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_payload_version() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded[0] = 0x02; // payload_version low byte, little-endian
        assert_eq!(
            StakePayloadV1::decode(&encoded),
            Err(StateError::UnsupportedStakePayloadVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded.push(0x00);
        assert!(matches!(
            StakePayloadV1::decode(&encoded),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
