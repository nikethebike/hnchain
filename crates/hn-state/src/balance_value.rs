use hn_hncs::{Decoder, write_u16, write_u128};

use crate::error::{StateError, StateResult};

/// `balance_version` for the current `BalanceValueV1` shape
/// (account-state.md §4.3, "Decided: balance value schema").
pub const BALANCE_VERSION_1: u16 = 1;

/// The `accounts` domain balance leaf value (account-state.md §4.3,
/// ADR-0007 SectionId `0x02`): the account's native HNCOIN balance only.
/// Native currency is a singleton per account, unlike non-native assets
/// ([`crate::asset_value::AssetValueV1`]), which form a
/// variable-cardinality collection stored in a separate leaf.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BalanceValueV1 {
    /// The account's native HNCOIN balance. `u128` for
    /// intermediate-computation overflow headroom; this is a
    /// storage-width decision only, not a supply, allocation, or fee
    /// decision.
    pub native_balance: u128,
}

impl BalanceValueV1 {
    /// Encodes this value as canonical HNCS bytes (account-state.md
    /// §4.3).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + 16);
        write_u16(&mut out, BALANCE_VERSION_1);
        write_u128(&mut out, self.native_balance);
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`BalanceValueV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let balance_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if balance_version != BALANCE_VERSION_1 {
            return Err(StateError::UnsupportedBalanceVersion {
                value: balance_version,
            });
        }

        let native_balance = decoder.read_u128().map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { native_balance })
    }
}

#[cfg(test)]
mod tests {
    use super::{BalanceValueV1, StateError};

    fn sample() -> BalanceValueV1 {
        BalanceValueV1 {
            native_balance: 0x0123_4567_89ab_cdef_0011_2233_4455_6677,
        }
    }

    #[test]
    fn encodes_matching_independent_oracle() {
        let encoded = sample().encode();
        assert_eq!(hex(&encoded), "01007766554433221100efcdab8967452301");
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        let encoded = sample().encode();
        let decoded = BalanceValueV1::decode(&encoded)?;
        assert_eq!(decoded, sample());
        Ok(())
    }

    #[test]
    fn rejects_unsupported_balance_version() {
        let mut encoded = sample().encode();
        encoded[0] = 0x02; // balance_version low byte, little-endian
        assert_eq!(
            BalanceValueV1::decode(&encoded),
            Err(StateError::UnsupportedBalanceVersion { value: 2 })
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut encoded = sample().encode();
        encoded.push(0xff);
        assert!(BalanceValueV1::decode(&encoded).is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
