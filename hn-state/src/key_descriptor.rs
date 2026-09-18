use hn_crypto::{
    ED25519_ALGORITHM_ID, ED25519_PUBLIC_KEY_LEN, KeyDescriptor, KeyRole, PUBLIC_KEY_MAX_LEN,
};
use hn_hncs::{Decoder, HncsResult, write_bytes, write_u16};

use crate::error::{StateError, StateResult};

/// Appends `key`'s canonical encoding (`algorithm_id: u16` + bounded
/// `public_key` bytes) to `out`. Shared by every wire location that
/// carries a [`KeyDescriptor`] the same way:
/// `ValidatorRecordV1.consensus_key`/`ValidatorUpdatePayloadV1.
/// new_consensus_key` (ADR-0006, `validator_consensus` role) and
/// `MultisigConfigV1.authorized_keys` (ADR-0026, `account_signing`
/// role). `key_role` itself is never part of this encoding — every
/// caller already knows the role from context (which field it is), the
/// same reasoning `ValidatorRecordV1`'s own documentation already gives
/// for not storing `consensus_key`'s role separately.
///
/// `HncsResult`-typed, not `StateResult`: encoding a `KeyDescriptor` can
/// only ever fail with a byte-framing error (the length field), never a
/// domain-specific one, so this can be called directly wherever a
/// `HncsResult`-typed closure is expected (unlike
/// [`decode_key_descriptor`], which cannot be — see
/// `ValidatorUpdatePayloadV1::decode`'s own documentation for why, and
/// `MultisigConfigV1`'s own hand-rolled authorized-keys decode loop for
/// the same reason applied to a bounded collection).
pub(crate) fn encode_key_descriptor(out: &mut Vec<u8>, key: &KeyDescriptor) -> HncsResult<()> {
    write_u16(out, key.algorithm_id());
    write_bytes(out, &key.public_key_bytes(), PUBLIC_KEY_MAX_LEN)
}

/// Decodes a [`KeyDescriptor`] written by [`encode_key_descriptor`], as
/// a `key_role` the caller supplies rather than one carried on the wire
/// (mirroring [`encode_key_descriptor`]'s own reasoning for not encoding
/// it).
pub(crate) fn decode_key_descriptor(
    decoder: &mut Decoder<'_>,
    key_role: KeyRole,
) -> StateResult<KeyDescriptor> {
    let algorithm_id = decoder.read_u16().map_err(StateError::Encoding)?;
    if algorithm_id != ED25519_ALGORITHM_ID {
        return Err(StateError::UnsupportedKeyAlgorithm {
            value: algorithm_id,
        });
    }
    let public_key_bytes = decoder
        .read_bytes(PUBLIC_KEY_MAX_LEN)
        .map_err(StateError::Encoding)?;
    let public_key: [u8; ED25519_PUBLIC_KEY_LEN] =
        public_key_bytes
            .try_into()
            .map_err(|_| StateError::UnsupportedKeyAlgorithm {
                value: algorithm_id,
            })?;
    KeyDescriptor::from_public_key_bytes(key_role, public_key)
        .map_err(StateError::InvalidConsensusKey)
}
