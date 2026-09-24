use hn_crypto::{KeyDescriptor, KeyRole};
use hn_hncs::{Decoder, write_u16};

use crate::error::{StateError, StateResult};
use crate::key_descriptor::{decode_key_descriptor, encode_key_descriptor};
use crate::permission_value::MultisigConfigV1;

/// `payload_version = 1` (ADR-0026, "Decided: `permission_update`
/// payload"): [`PermissionUpdatePayloadV1::SetAccountSigningMultisig`],
/// wire-unchanged since ADR-0026 — a bare [`MultisigConfigV1`], no
/// discriminant prefix.
pub const PERMISSION_UPDATE_PAYLOAD_VERSION_1: u16 = 1;

/// `payload_version = 2` (ADR-0028, "Account-Level Key Rotation"):
/// [`PermissionUpdatePayloadV1::RotateIdentityKey`] — a bare
/// `KeyDescriptorV1`, no discriminant prefix. `payload_version` itself
/// is what distinguishes the two operations (see the type's own
/// documentation for why a separate operation byte would be redundant
/// on top of it); a version, not a field, is this payload's
/// discriminant.
pub const PERMISSION_UPDATE_PAYLOAD_VERSION_2: u16 = 2;

/// `permission_update` (`tx_type = 0x08`) payload: two operations,
/// distinguished by `payload_version` rather than a discriminant field
/// (ADR-0026 for the first, ADR-0028 for the second) — mirrors
/// `hn_crypto::SignatureEnvelope`'s own `envelope_version` 1-vs-2 split
/// exactly. A version, not a byte alongside it, is the discriminant
/// because the two operations have completely disjoint wire shapes and
/// there is no case where knowing the version still leaves the
/// operation ambiguous — unlike `ValidatorUpdatePayloadV1`/
/// `GovernancePayloadV1`, whose several operations all share one fixed
/// `payload_version` and genuinely need a separate byte to disambiguate.
///
/// Whether a given `SetAccountSigningMultisig` is a first activation or
/// a reconfiguration, and whether a `RotateIdentityKey` is even valid
/// (requires an existing `IdentityValueV1` and no active multisig
/// configuration, ADR-0028) are transaction-validation concerns this
/// payload's own decode/encode does not enforce — see ADR-0026's/
/// ADR-0028's own "Decided" text for the full authorization rules.
///
/// Deactivating an active multisig configuration back to single-key
/// mode is still not representable by either operation — ADR-0028's own
/// "Open Decisions" names why: it needs multisig-threshold
/// authorization for a successor key, a distinct mechanism from either
/// operation here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PermissionUpdatePayloadV1 {
    /// Sets the sender's `account_signing_multisig` configuration
    /// (ADR-0026).
    SetAccountSigningMultisig(MultisigConfigV1),
    /// Replaces the sender's `IdentityValueV1.key` (ADR-0028) — valid
    /// only for an account with no active multisig configuration.
    RotateIdentityKey(KeyDescriptor),
}

impl PermissionUpdatePayloadV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            Self::SetAccountSigningMultisig(config) => {
                write_u16(&mut out, PERMISSION_UPDATE_PAYLOAD_VERSION_1);
                config.encode_into(&mut out)?;
            }
            Self::RotateIdentityKey(key) => {
                write_u16(&mut out, PERMISSION_UPDATE_PAYLOAD_VERSION_2);
                encode_key_descriptor(&mut out, key).map_err(StateError::Encoding)?;
            }
        }
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`PermissionUpdatePayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let payload_version = decoder.read_u16().map_err(StateError::Encoding)?;
        let value = match payload_version {
            PERMISSION_UPDATE_PAYLOAD_VERSION_1 => {
                let config = MultisigConfigV1::decode_from(&mut decoder)?;
                Self::SetAccountSigningMultisig(config)
            }
            PERMISSION_UPDATE_PAYLOAD_VERSION_2 => {
                let key = decode_key_descriptor(&mut decoder, KeyRole::AccountSigning)?;
                Self::RotateIdentityKey(key)
            }
            value => {
                return Err(StateError::UnsupportedPermissionUpdatePayloadVersion { value });
            }
        };
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::{
        PERMISSION_UPDATE_PAYLOAD_VERSION_1, PERMISSION_UPDATE_PAYLOAD_VERSION_2,
        PermissionUpdatePayloadV1, StateError,
    };
    use crate::error::StateResult;
    use crate::permission_value::MultisigConfigV1;

    fn set_multisig_payload() -> PermissionUpdatePayloadV1 {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x01; 32]);
        PermissionUpdatePayloadV1::SetAccountSigningMultisig(MultisigConfigV1 {
            threshold: 1,
            authorized_keys: vec![keypair.key_descriptor()],
        })
    }

    fn rotate_key_payload() -> PermissionUpdatePayloadV1 {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x02; 32]);
        PermissionUpdatePayloadV1::RotateIdentityKey(keypair.key_descriptor())
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        for value in [set_multisig_payload(), rotate_key_payload()] {
            let decoded = PermissionUpdatePayloadV1::decode(&value.encode()?)?;
            assert_eq!(decoded, value);
        }
        Ok(())
    }

    #[test]
    fn set_multisig_encodes_as_version_1() -> StateResult<()> {
        let encoded = set_multisig_payload().encode()?;
        assert_eq!(
            &encoded[0..2],
            &PERMISSION_UPDATE_PAYLOAD_VERSION_1.to_le_bytes()
        );
        Ok(())
    }

    #[test]
    fn rotate_key_encodes_as_version_2() -> StateResult<()> {
        let encoded = rotate_key_payload().encode()?;
        assert_eq!(
            &encoded[0..2],
            &PERMISSION_UPDATE_PAYLOAD_VERSION_2.to_le_bytes()
        );
        Ok(())
    }

    #[test]
    fn rejects_unsupported_payload_version() -> StateResult<()> {
        let mut encoded = set_multisig_payload().encode()?;
        encoded[0] = 0x03; // payload_version low byte, little-endian
        assert_eq!(
            PermissionUpdatePayloadV1::decode(&encoded),
            Err(StateError::UnsupportedPermissionUpdatePayloadVersion { value: 3 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        for value in [set_multisig_payload(), rotate_key_payload()] {
            let mut encoded = value.encode()?;
            encoded.push(0x00);
            assert!(matches!(
                PermissionUpdatePayloadV1::decode(&encoded),
                Err(StateError::Encoding(_))
            ));
        }
        Ok(())
    }
}
