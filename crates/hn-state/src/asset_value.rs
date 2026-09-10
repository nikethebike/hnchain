use hn_hncs::{Decoder, write_u16, write_u128};

use crate::error::{StateError, StateResult};

/// `asset_version` for the current `AssetValueV1` shape (account-state.md
/// §4.7, "Decided: asset value schema").
pub const ASSET_VERSION_1: u16 = 1;

/// Maximum number of distinct `asset_id` holdings one account's asset
/// leaf may carry. An implementation-level resource bound, not a
/// consensus/economic limit on how many asset classes may exist.
pub const MAX_ASSET_HOLDINGS: usize = 1024;

/// The `accounts` domain asset leaf value (account-state.md §4.7,
/// ADR-0007 SectionId `0x06`): the account's non-native (protocol-level,
/// bridged) asset holdings. A variable-cardinality collection, unlike
/// native currency ([`crate::balance_value::BalanceValueV1`]), which is
/// a singleton stored in a separate leaf. Absence of an `asset_id` from
/// `holdings` means a zero balance; there is no explicit zero-amount
/// entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetValueV1 {
    /// `asset_id -> amount` holdings. `asset_id` references a curated
    /// definition in ADR-0007's `assets` domain (`0x0005`); this leaf
    /// does not duplicate that definition, only the balance.
    pub holdings: Vec<(u16, u128)>,
}

impl AssetValueV1 {
    /// Encodes this value as canonical HNCS bytes (account-state.md
    /// §4.7).
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, ASSET_VERSION_1);
        hn_hncs::write_map(
            &mut out,
            &self.holdings,
            MAX_ASSET_HOLDINGS,
            |out, asset_id| {
                write_u16(out, *asset_id);
                Ok(())
            },
            |out, amount| {
                write_u128(out, *amount);
                Ok(())
            },
        )
        .map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`AssetValueV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let asset_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if asset_version != ASSET_VERSION_1 {
            return Err(StateError::UnsupportedAssetVersion {
                value: asset_version,
            });
        }

        let holdings = decoder
            .read_map(MAX_ASSET_HOLDINGS, Decoder::read_u16, Decoder::read_u128)
            .map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { holdings })
    }
}

#[cfg(test)]
mod tests {
    use super::{AssetValueV1, StateError};

    fn sample() -> AssetValueV1 {
        AssetValueV1 {
            holdings: vec![(5, 100), (2, 999_999_999_999)],
        }
    }

    #[test]
    fn encodes_matching_independent_oracle() -> crate::error::StateResult<()> {
        let encoded = sample().encode()?;
        assert_eq!(
            hex(&encoded),
            "0100020000000200ff0fa5d4e80000000000000000000000050064000000000000000000000000000000"
        );
        Ok(())
    }

    #[test]
    fn encodes_empty_holdings() -> crate::error::StateResult<()> {
        let encoded = AssetValueV1 { holdings: vec![] }.encode()?;
        assert_eq!(hex(&encoded), "010000000000");
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        let encoded = sample().encode()?;
        let mut decoded = AssetValueV1::decode(&encoded)?;
        decoded.holdings.sort();
        let mut expected = sample();
        expected.holdings.sort();
        assert_eq!(decoded, expected);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_asset_version() -> crate::error::StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded[0] = 0x02; // asset_version low byte, little-endian
        assert_eq!(
            AssetValueV1::decode(&encoded),
            Err(StateError::UnsupportedAssetVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> crate::error::StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded.push(0xff);
        assert!(AssetValueV1::decode(&encoded).is_err());
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
