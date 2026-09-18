use hn_crypto::{ED25519_PUBLIC_KEY_LEN, KeyDescriptor, KeyRole};
use hn_hncs::{Decoder, HncsError, validate_count, write_bool, write_set, write_u8, write_u16};

use crate::error::{StateError, StateResult};
use crate::key_descriptor::{decode_key_descriptor, encode_key_descriptor};

/// `permission_version` for the current `PermissionValueV1` shape
/// (ADR-0026, "Decided: `PermissionValueV1`").
pub const PERMISSION_VERSION_1: u16 = 1;

/// Maximum number of `authorized_keys` entries a [`MultisigConfigV1`] may
/// carry. An implementation resource bound (ADR-0026), the same class of
/// decision as [`crate::MAX_ACCESS_LIST_ENTRIES`]/
/// [`crate::MAX_ASSET_HOLDINGS`] — generous headroom over any realistic
/// custody committee size, not derived from a specific use case.
pub const MAX_AUTHORIZED_KEYS: usize = 16;

/// Account-level Permission state (`SectionId 0x04`, account-state.md
/// §4.5): the `account_signing` role's threshold/multisignature
/// configuration only (ADR-0026). The other 7 conceptual Permission
/// capabilities (administration, operation, viewing, voting delegation,
/// spending limits, session authorization, emergency lock or recovery)
/// are not represented — this section does not attempt to speak for
/// capabilities that have no decided schema yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionValueV1 {
    /// Absent (the default for every account) means single-key mode,
    /// unchanged from `account_signing`'s pre-ADR-0026 behavior. Present
    /// means every `account_signing`-role authorization for this account
    /// must meet [`MultisigConfigV1::threshold`], per ADR-0026's
    /// "Decided: Multi-signature verification rule."
    pub account_signing_multisig: Option<MultisigConfigV1>,
}

/// The `account_signing` role's threshold/multisignature configuration
/// (ADR-0026, "Decided: `PermissionValueV1`").
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultisigConfigV1 {
    /// How many of `authorized_keys` must each contribute a valid,
    /// distinct signature. `1 <= threshold <= authorized_keys.len()`,
    /// checked on decode ([`StateError::InvalidMultisigThreshold`]).
    pub threshold: u8,
    /// The account's authorized `account_signing` keys. Canonical order
    /// is ascending by encoded bytes (algorithm_id then public_key,
    /// HNCS `set` semantics — [`hn_hncs::write_set`]/duplicate rejection,
    /// not an unordered collection like `AssetValueV1`'s map: order is
    /// meaningful here, since a `SignatureEnvelope.key_reference`
    /// (ADR-0026) indexes into it). Bounded by [`MAX_AUTHORIZED_KEYS`].
    pub authorized_keys: Vec<KeyDescriptor>,
}

impl PermissionValueV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, PERMISSION_VERSION_1);
        write_bool(&mut out, self.account_signing_multisig.is_some());
        if let Some(config) = &self.account_signing_multisig {
            config.encode_into(&mut out)?;
        }
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`PermissionValueV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let permission_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if permission_version != PERMISSION_VERSION_1 {
            return Err(StateError::UnsupportedPermissionVersion {
                value: permission_version,
            });
        }

        let has_config = decoder.read_bool().map_err(StateError::Encoding)?;
        let account_signing_multisig = if has_config {
            Some(MultisigConfigV1::decode_from(&mut decoder)?)
        } else {
            None
        };

        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            account_signing_multisig,
        })
    }
}

impl MultisigConfigV1 {
    /// Appends this value's canonical HNCS bytes to `out`. Shared by
    /// [`PermissionValueV1::encode`] and
    /// `PermissionUpdatePayloadV1::encode`, both of which carry a
    /// `MultisigConfigV1` on the wire the same way.
    pub(crate) fn encode_into(&self, out: &mut Vec<u8>) -> StateResult<()> {
        self.validate()?;
        write_u8(out, self.threshold);
        write_set(
            out,
            &self.authorized_keys,
            MAX_AUTHORIZED_KEYS,
            encode_key_descriptor,
        )
        .map_err(StateError::Encoding)
    }

    /// Decodes this value's fields from `decoder`, without requiring the
    /// decoder to be exhausted afterward — shared the same way
    /// [`MultisigConfigV1::encode_into`] is.
    ///
    /// Hand-rolled `u32 count || elements` with inline canonical-order
    /// and duplicate checking, not `hn_hncs::read_set`: each element's
    /// decode can fail with a domain-specific error
    /// ([`StateError::UnsupportedKeyAlgorithm`]/
    /// [`StateError::InvalidConsensusKey`]) that `read_set`'s
    /// `HncsResult`-typed closure cannot express — the same reason
    /// `QuorumCertificate.aggregate_proof` (ADR-0012) and
    /// `ValidatorUpdatePayloadV1.new_consensus_key` (ADR-0006) each
    /// hand-roll their own decode loop instead of using a generic HNCS
    /// collection helper.
    ///
    /// Order/duplicate checking compares each entry's raw
    /// `public_key_bytes()` directly rather than its full re-encoded
    /// byte span: sound only because every active key shares the same
    /// `algorithm_id` and the same encoded length prefix today (Ed25519
    /// is this profile's only active algorithm, ADR-0002) — those two
    /// components never actually differ between entries, so comparing
    /// public-key bytes alone is exactly equivalent to comparing full
    /// encoded bytes, matching [`hn_hncs::write_set`]'s own canonical
    /// order on the encode side. A second active algorithm would need
    /// this comparison broadened, not assumed to still hold.
    pub(crate) fn decode_from(decoder: &mut Decoder<'_>) -> StateResult<Self> {
        let threshold = decoder.read_u8().map_err(StateError::Encoding)?;

        let count = decoder.read_u32().map_err(StateError::Encoding)? as usize;
        validate_count(count, MAX_AUTHORIZED_KEYS).map_err(StateError::Encoding)?;
        let mut authorized_keys = Vec::with_capacity(count);
        let mut previous: Option<[u8; ED25519_PUBLIC_KEY_LEN]> = None;
        for _ in 0..count {
            let key = decode_key_descriptor(decoder, KeyRole::AccountSigning)?;
            let public_key = key.public_key_bytes();
            if let Some(previous) = previous {
                match previous.cmp(&public_key) {
                    core::cmp::Ordering::Greater => {
                        return Err(StateError::Encoding(HncsError::UnsortedSet));
                    }
                    core::cmp::Ordering::Equal => {
                        return Err(StateError::Encoding(HncsError::DuplicateSetElement));
                    }
                    core::cmp::Ordering::Less => {}
                }
            }
            previous = Some(public_key);
            authorized_keys.push(key);
        }

        let config = Self {
            threshold,
            authorized_keys,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> StateResult<()> {
        if self.threshold == 0 || usize::from(self.threshold) > self.authorized_keys.len() {
            return Err(StateError::InvalidMultisigThreshold {
                threshold: self.threshold,
                authorized_key_count: self.authorized_keys.len(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::{MultisigConfigV1, PermissionValueV1};
    use crate::error::{StateError, StateResult};

    fn key(seed: u8) -> hn_crypto::KeyDescriptor {
        Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32]).key_descriptor()
    }

    fn single_key_mode() -> PermissionValueV1 {
        PermissionValueV1 {
            account_signing_multisig: None,
        }
    }

    fn two_of_three() -> PermissionValueV1 {
        // Already in canonical (ascending public-key-bytes) order, so
        // round-tripping through decode produces a structurally equal
        // value -- `encode_canonicalizes_authorized_keys_order` below
        // covers the out-of-order input case separately.
        let mut authorized_keys = vec![key(0x01), key(0x02), key(0x03)];
        authorized_keys.sort_by_key(hn_crypto::KeyDescriptor::public_key_bytes);
        PermissionValueV1 {
            account_signing_multisig: Some(MultisigConfigV1 {
                threshold: 2,
                authorized_keys,
            }),
        }
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        for value in [single_key_mode(), two_of_three()] {
            let decoded = PermissionValueV1::decode(&value.encode()?)?;
            assert_eq!(decoded, value);
        }
        Ok(())
    }

    #[test]
    fn encode_canonicalizes_authorized_keys_order() -> StateResult<()> {
        // Constructed out of order; encode must sort, decode must accept
        // the sorted result and round-trip to the same sorted value.
        let out_of_order = PermissionValueV1 {
            account_signing_multisig: Some(MultisigConfigV1 {
                threshold: 1,
                authorized_keys: vec![key(0x03), key(0x01), key(0x02)],
            }),
        };
        let decoded = PermissionValueV1::decode(&out_of_order.encode()?)?;
        assert!(decoded.account_signing_multisig.is_some());
        if let Some(config) = decoded.account_signing_multisig {
            let keys: Vec<[u8; 32]> = config
                .authorized_keys
                .iter()
                .map(hn_crypto::KeyDescriptor::public_key_bytes)
                .collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted);
        }
        Ok(())
    }

    #[test]
    fn rejects_zero_threshold() {
        let mut value = two_of_three();
        if let Some(config) = value.account_signing_multisig.as_mut() {
            config.threshold = 0;
        }
        let encoded = value.encode();
        assert!(matches!(
            encoded,
            Err(StateError::InvalidMultisigThreshold { .. })
        ));
    }

    #[test]
    fn rejects_threshold_exceeding_authorized_key_count() {
        let mut value = two_of_three();
        if let Some(config) = value.account_signing_multisig.as_mut() {
            config.threshold = 4;
        }
        let encoded = value.encode();
        assert!(matches!(
            encoded,
            Err(StateError::InvalidMultisigThreshold { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_authorized_keys() {
        let value = PermissionValueV1 {
            account_signing_multisig: Some(MultisigConfigV1 {
                threshold: 1,
                authorized_keys: vec![key(0x01), key(0x01)],
            }),
        };
        assert!(matches!(
            value.encode(),
            Err(StateError::Encoding(
                hn_hncs::HncsError::DuplicateSetElement
            ))
        ));
    }

    #[test]
    fn rejects_unsupported_permission_version() -> StateResult<()> {
        let mut encoded = single_key_mode().encode()?;
        encoded[0] = 0x02; // permission_version low byte, little-endian
        assert_eq!(
            PermissionValueV1::decode(&encoded),
            Err(StateError::UnsupportedPermissionVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = single_key_mode().encode()?;
        encoded.push(0x00);
        assert!(matches!(
            PermissionValueV1::decode(&encoded),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }
}
