use hn_crypto::Digest;
use hn_hncs::{Decoder, write_fixed_bytes, write_u8, write_u16};

use crate::error::{StateError, StateResult};

/// `envelope_version` for the current `EnvelopeValueV1` shape
/// (account-state.md §4.1, "Decided: envelope value schema"). A
/// Structure Version in ADR-0022's sense: it changes only when the
/// envelope's own shape changes, independently of any section's own
/// `*_version` field inside [`SectionVersionsV1`].
pub const ENVELOPE_VERSION_1: u16 = 1;

/// The `account_type` registry (account-state.md §3.1, segregated
/// model). `0x00` is reserved and never a valid encoded value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AccountType {
    /// The only `account_type` currently defined.
    Standard = 0x01,
}

impl AccountType {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::Standard),
            _ => Err(StateError::InvalidAccountType { value }),
        }
    }

    const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Per-section structure versions carried by the envelope: one `u16`
/// per required section other than the envelope itself, in the fixed
/// field order account-state.md §4.1 defines (account-state.md §4:
/// "Required Sections" are all mandatory, so there is no sparse/missing
/// case and no need for map machinery).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionVersionsV1 {
    /// Identity section structure version (account-state.md §3.2).
    pub identity_version: u16,
    /// Balance section structure version (account-state.md §4.3).
    pub balance_version: u16,
    /// Nonce section structure version (account-state.md §4.4).
    pub nonce_version: u16,
    /// Permission section structure version (account-state.md §4.5).
    pub permission_version: u16,
    /// Metadata section structure version (account-state.md §4.6).
    pub metadata_version: u16,
    /// Asset section structure version (account-state.md §4.7).
    pub asset_version: u16,
    /// Lifecycle section structure version (account-state.md §4.9).
    pub lifecycle_version: u16,
}

/// The `accounts` domain envelope leaf value (account-state.md §4.1,
/// ADR-0007 SectionId `0x00`): `envelope_version` is written as the
/// fixed constant [`ENVELOPE_VERSION_1`], not carried as a field, since
/// this type only ever represents that one shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvelopeValueV1 {
    /// This account's `account_type`.
    pub account_type: AccountType,
    /// This account's own 32-byte `address_body`
    /// (`hn_crypto::account_address_body`'s output), stored raw since
    /// `state_key` is a one-way hash a reader cannot invert.
    pub address: Digest,
    /// Structure versions for every other required section.
    pub section_versions: SectionVersionsV1,
}

impl EnvelopeValueV1 {
    /// Encodes this value as canonical HNCS bytes
    /// (account-state.md §4.1).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + 1 + 32 + 7 * 2);
        write_u16(&mut out, ENVELOPE_VERSION_1);
        write_u8(&mut out, self.account_type.as_u8());
        write_fixed_bytes(&mut out, &self.address);
        write_u16(&mut out, self.section_versions.identity_version);
        write_u16(&mut out, self.section_versions.balance_version);
        write_u16(&mut out, self.section_versions.nonce_version);
        write_u16(&mut out, self.section_versions.permission_version);
        write_u16(&mut out, self.section_versions.metadata_version);
        write_u16(&mut out, self.section_versions.asset_version);
        write_u16(&mut out, self.section_versions.lifecycle_version);
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`EnvelopeValueV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let envelope_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if envelope_version != ENVELOPE_VERSION_1 {
            return Err(StateError::UnsupportedEnvelopeVersion {
                value: envelope_version,
            });
        }

        let account_type = AccountType::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let address = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;

        let section_versions = SectionVersionsV1 {
            identity_version: decoder.read_u16().map_err(StateError::Encoding)?,
            balance_version: decoder.read_u16().map_err(StateError::Encoding)?,
            nonce_version: decoder.read_u16().map_err(StateError::Encoding)?,
            permission_version: decoder.read_u16().map_err(StateError::Encoding)?,
            metadata_version: decoder.read_u16().map_err(StateError::Encoding)?,
            asset_version: decoder.read_u16().map_err(StateError::Encoding)?,
            lifecycle_version: decoder.read_u16().map_err(StateError::Encoding)?,
        };

        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            account_type,
            address,
            section_versions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AccountType, EnvelopeValueV1, SectionVersionsV1};
    use crate::error::StateError;

    const ACCOUNT_ADDRESS: [u8; 32] = [
        0xd0, 0x40, 0xe6, 0xd2, 0xad, 0x41, 0xfb, 0xbe, 0x91, 0xc3, 0xa2, 0x19, 0x26, 0x42, 0xa9,
        0xe4, 0x39, 0x6c, 0x5c, 0x5e, 0x85, 0xff, 0xe3, 0xf1, 0x90, 0xd1, 0x0a, 0xba, 0x66, 0xd7,
        0xdf, 0x7a,
    ];

    fn sample() -> EnvelopeValueV1 {
        EnvelopeValueV1 {
            account_type: AccountType::Standard,
            address: ACCOUNT_ADDRESS,
            section_versions: SectionVersionsV1 {
                identity_version: 1,
                balance_version: 1,
                nonce_version: 1,
                permission_version: 1,
                metadata_version: 1,
                asset_version: 1,
                lifecycle_version: 1,
            },
        }
    }

    #[test]
    fn encodes_matching_independent_oracle() {
        let encoded = sample().encode();
        assert_eq!(
            hex(&encoded),
            "010001d040e6d2ad41fbbe91c3a2192642a9e4396c5c5e85ffe3f190d10aba66d7df7a\
             0100010001000100010001000100"
        );
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        let encoded = sample().encode();
        let decoded = EnvelopeValueV1::decode(&encoded)?;
        assert_eq!(decoded, sample());
        Ok(())
    }

    #[test]
    fn rejects_unsupported_envelope_version() {
        let mut encoded = sample().encode();
        encoded[0] = 0x02; // envelope_version low byte, little-endian
        assert_eq!(
            EnvelopeValueV1::decode(&encoded),
            Err(StateError::UnsupportedEnvelopeVersion { value: 2 })
        );
    }

    #[test]
    fn rejects_reserved_account_type() {
        let mut encoded = sample().encode();
        encoded[2] = 0x00; // account_type byte
        assert_eq!(
            EnvelopeValueV1::decode(&encoded),
            Err(StateError::InvalidAccountType { value: 0 })
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut encoded = sample().encode();
        encoded.push(0xff);
        assert!(EnvelopeValueV1::decode(&encoded).is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
