use hn_crypto::{KeyDescriptor, KeyRole};
use hn_hncs::{Decoder, write_u16};

use crate::error::{StateError, StateResult};
use crate::key_descriptor::{decode_key_descriptor, encode_key_descriptor};

/// `identity_version` for the current `IdentityValueV1` shape
/// (ADR-0027, "Decided: `IdentityValueV1`").
pub const IDENTITY_VERSION_1: u16 = 1;

/// Identity State (`SectionId 0x01`, account-state.md §3.2): the
/// account's currently-active `account_signing` key (ADR-0027,
/// "Identity State And Account Key Bootstrap").
///
/// `key.algorithm_id()`/`key.public_key_bytes()` are the wire-level
/// `algorithm_id`/`public_key` fields ADR-0027's own schema names —
/// stored as a [`KeyDescriptor`] rather than two raw fields for the
/// same reason `ValidatorRecordV1.consensus_key` already is: it is the
/// type every signature-verification call site in this project already
/// expects. `key_role` itself is never part of the encoding — it is
/// always `AccountSigning` by construction (this is the Identity
/// section, not a generic key store), the same reasoning
/// [`crate::key_descriptor::encode_key_descriptor`] already documents
/// for not storing `key_role` anywhere on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityValueV1 {
    /// The account's currently-active `account_signing` key.
    pub key: KeyDescriptor,
}

impl IdentityValueV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, IDENTITY_VERSION_1);
        encode_key_descriptor(&mut out, &self.key).map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`IdentityValueV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let identity_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if identity_version != IDENTITY_VERSION_1 {
            return Err(StateError::UnsupportedIdentityVersion {
                value: identity_version,
            });
        }

        let key = decode_key_descriptor(&mut decoder, KeyRole::AccountSigning)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { key })
    }
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::IdentityValueV1;
    use crate::error::{StateError, StateResult};

    fn sample() -> IdentityValueV1 {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x01; 32]);
        IdentityValueV1 {
            key: keypair.key_descriptor(),
        }
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let value = sample();
        let decoded = IdentityValueV1::decode(&value.encode()?)?;
        assert_eq!(decoded, value);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_identity_version() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded[0] = 0x02; // identity_version low byte, little-endian
        assert_eq!(
            IdentityValueV1::decode(&encoded),
            Err(StateError::UnsupportedIdentityVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = sample().encode()?;
        encoded.push(0x00);
        assert!(matches!(
            IdentityValueV1::decode(&encoded),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }
}
