use hn_hncs::{Decoder, write_u16};

use crate::error::{StateError, StateResult};
use crate::permission_value::MultisigConfigV1;

/// `payload_version` for the current `permission_update` (`tx_type =
/// 0x08`) payload shape (ADR-0026, "Decided: `permission_update`
/// payload").
pub const PERMISSION_UPDATE_PAYLOAD_VERSION_1: u16 = 1;

/// `permission_update` (`tx_type = 0x08`) payload: sets the sender's
/// `account_signing_multisig` configuration to
/// `new_account_signing_multisig` (ADR-0026).
///
/// One operation only, not a discriminated multi-operation payload like
/// `ValidatorUpdatePayloadV1`/`GovernancePayloadV1`: this pass has
/// exactly one real operation ("set the configuration"), not several
/// sharing one `tx_type` slot. Whether a given `permission_update` is a
/// first activation or a reconfiguration is determined by the sender's
/// *current* stored state, not by this payload — and correspondingly,
/// which authorization rule applies (today's single-key default for a
/// first activation, or [`crate::verify_multisig_authorization`] against
/// the pre-transaction configuration for a reconfiguration) is a
/// transaction-validation concern this payload's own decode/encode does
/// not enforce; see ADR-0026's own "Decided: `permission_update`
/// payload" for the full authorization rule.
///
/// Deactivating an active configuration back to single-key mode is not
/// representable by this payload — `new_account_signing_multisig` is
/// mandatory, never absent. ADR-0026's own "Explicitly Not Resolved"
/// names why: doing so needs Identity State's own still-undecided
/// `active_key(...)` resolution mechanism (ADR-0002), which this ADR
/// does not touch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionUpdatePayloadV1 {
    /// The `account_signing_multisig` configuration to set.
    pub new_account_signing_multisig: MultisigConfigV1,
}

impl PermissionUpdatePayloadV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, PERMISSION_UPDATE_PAYLOAD_VERSION_1);
        self.new_account_signing_multisig.encode_into(&mut out)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`PermissionUpdatePayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let payload_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if payload_version != PERMISSION_UPDATE_PAYLOAD_VERSION_1 {
            return Err(StateError::UnsupportedPermissionUpdatePayloadVersion {
                value: payload_version,
            });
        }

        let new_account_signing_multisig = MultisigConfigV1::decode_from(&mut decoder)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            new_account_signing_multisig,
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::{PERMISSION_UPDATE_PAYLOAD_VERSION_1, PermissionUpdatePayloadV1, StateError};
    use crate::error::StateResult;
    use crate::permission_value::MultisigConfigV1;

    fn payload() -> PermissionUpdatePayloadV1 {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x01; 32]);
        PermissionUpdatePayloadV1 {
            new_account_signing_multisig: MultisigConfigV1 {
                threshold: 1,
                authorized_keys: vec![keypair.key_descriptor()],
            },
        }
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let value = payload();
        let decoded = PermissionUpdatePayloadV1::decode(&value.encode()?)?;
        assert_eq!(decoded, value);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_payload_version() -> StateResult<()> {
        let mut encoded = payload().encode()?;
        encoded[0] = (PERMISSION_UPDATE_PAYLOAD_VERSION_1 + 1) as u8;
        assert_eq!(
            PermissionUpdatePayloadV1::decode(&encoded),
            Err(StateError::UnsupportedPermissionUpdatePayloadVersion {
                value: PERMISSION_UPDATE_PAYLOAD_VERSION_1 + 1
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = payload().encode()?;
        encoded.push(0x00);
        assert!(matches!(
            PermissionUpdatePayloadV1::decode(&encoded),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }
}
