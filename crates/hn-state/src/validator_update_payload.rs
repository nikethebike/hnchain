use hn_crypto::KeyDescriptor;
use hn_hncs::{Decoder, write_bool, write_u8, write_u16};

use crate::error::{StateError, StateResult};
use crate::validator_record::{decode_key_descriptor, encode_key_descriptor};

/// `payload_version` for the current `ValidatorUpdatePayloadV1` shape
/// (ADR-0006, "Decided: `stake`/`unstake`/`validator_update` payload
/// shapes").
pub const VALIDATOR_UPDATE_PAYLOAD_VERSION_1: u16 = 1;

/// The `operation` registry (ADR-0006, "Decided: `stake`/`unstake`/
/// `validator_update` payload shapes"), closed for this payload version.
/// Folds 5 of `validator-set.md` §11's 8 conceptual operations into
/// this one `tx_type` — ADR-0006's `tx_type` registry is already closed
/// for `tx_version = 1`, with exactly one slot available for everything
/// validator-lifecycle-shaped that is not `stake`/`unstake`
/// (`validator_bond`/`validator_unbond`, which map directly to those
/// instead). `validator_update_metadata` is excluded, not merely
/// unassigned a number: `ValidatorRecordV1` has no `metadata_hash`
/// field yet (blocked, no driver — the same deferral account-state.md's
/// own Metadata section already has).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ValidatorOperation {
    /// Creates a new `ValidatorRecordV1` for `sender`
    /// (`validator_id = sender`, `bonded_stake = 0`, `voting_power = 0`,
    /// `status = Registered`). Carries `new_consensus_key`.
    Register = 0x01,
    /// The explicit opt-in transition ADR-0010's "Decided: admission
    /// mechanism" already specifies the timing and reasoning for
    /// (epoch-delayed, opt-in rather than automatic).
    Activate = 0x02,
    /// The explicit opt-out counterpart to [`Self::Activate`].
    Deactivate = 0x03,
    /// The permanent counterpart to [`Self::Deactivate`].
    Exit = 0x04,
    /// Replaces `consensus_key`. Exact activation-epoch/old-key-
    /// validity-window mechanics stay owned by ADR-0010's "Key
    /// Rotation," not decided by this payload. Carries
    /// `new_consensus_key`.
    UpdateKeys = 0x05,
}

impl ValidatorOperation {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::Register),
            0x02 => Ok(Self::Activate),
            0x03 => Ok(Self::Deactivate),
            0x04 => Ok(Self::Exit),
            0x05 => Ok(Self::UpdateKeys),
            _ => Err(StateError::InvalidValidatorOperation { value }),
        }
    }

    const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Whether this operation carries `new_consensus_key`.
    const fn requires_consensus_key(self) -> bool {
        matches!(self, Self::Register | Self::UpdateKeys)
    }
}

/// `validator_update` (`tx_type = 0x06`) payload: a discriminated
/// operation on the sender's own `ValidatorRecordV1` (ADR-0006,
/// "Decided: `stake`/`unstake`/`validator_update` payload shapes").
///
/// No `validator_id` field, for the same reason `StakePayloadV1` has
/// none: `validator_id` is always `sender` (ADR-0010, "Decided:
/// `validator_id` derivation"), which also *is* the authorization check
/// — no separate ownership field needed.
///
/// `operation: u8` followed directly by `new_consensus_key`, not an
/// outer wrapper around a per-operation sub-message: this project's
/// established discriminated-encoding style (`VoteSigningPayloadV1`'s
/// `target_type`/`target_hash` pair is the precedent) is a plain
/// discriminant plus conditional fields, not boxed variants.
/// `new_consensus_key` itself *does* use `hn_hncs`'s generic `optional`
/// concept (a presence flag, not a canonical-sentinel-value convention
/// like `target_hash`'s all-zero `Nil` encoding) — but hand-rolled, not
/// `write_optional`/`read_optional`: those helpers require their inner
/// closure to return `HncsResult`, and
/// [`crate::validator_record::decode_key_descriptor`] can fail with a
/// domain-specific [`hn_crypto::IdentityError`]-adjacent
/// [`StateError`] (unsupported algorithm, invalid key), which
/// `HncsResult` cannot express — the exact same reason
/// `QuorumCertificate.aggregate_proof` (ADR-0012) could not use
/// `write_list`/`read_list` either.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatorUpdatePayloadV1 {
    /// Which operation this payload performs.
    pub operation: ValidatorOperation,
    /// Present if and only if `operation` is [`ValidatorOperation::Register`]
    /// or [`ValidatorOperation::UpdateKeys`] — enforced on decode, not
    /// just by convention, so there is exactly one canonical encoding
    /// per operation (mirroring [`crate::VoteSigningPayloadV1::decode`]'s
    /// nil-target rule).
    pub new_consensus_key: Option<KeyDescriptor>,
}

impl ValidatorUpdatePayloadV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, VALIDATOR_UPDATE_PAYLOAD_VERSION_1);
        write_u8(&mut out, self.operation.as_u8());
        write_bool(&mut out, self.new_consensus_key.is_some());
        if let Some(key) = &self.new_consensus_key {
            encode_key_descriptor(&mut out, key).map_err(StateError::Encoding)?;
        }
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ValidatorUpdatePayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let payload_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if payload_version != VALIDATOR_UPDATE_PAYLOAD_VERSION_1 {
            return Err(StateError::UnsupportedValidatorUpdatePayloadVersion {
                value: payload_version,
            });
        }

        let operation =
            ValidatorOperation::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let has_key = decoder.read_bool().map_err(StateError::Encoding)?;
        let new_consensus_key = if has_key {
            Some(decode_key_descriptor(&mut decoder)?)
        } else {
            None
        };

        decoder.finish().map_err(StateError::Encoding)?;

        if has_key != operation.requires_consensus_key() {
            return Err(StateError::ValidatorUpdateKeyPresenceMismatch);
        }

        Ok(Self {
            operation,
            new_consensus_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_crypto::{KeyDescriptor, KeyRole};

    use super::{StateError, ValidatorOperation, ValidatorUpdatePayloadV1};
    use crate::error::StateResult;

    const PUBLIC_KEY: [u8; 32] = [
        0xd0, 0x4a, 0xb2, 0x32, 0x74, 0x2b, 0xb4, 0xab, 0x3a, 0x13, 0x68, 0xbd, 0x46, 0x15, 0xe4,
        0xe6, 0xd0, 0x22, 0x4a, 0xb7, 0x1a, 0x01, 0x6b, 0xaf, 0x85, 0x20, 0xa3, 0x32, 0xc9, 0x77,
        0x87, 0x37,
    ];

    fn register() -> StateResult<ValidatorUpdatePayloadV1> {
        Ok(ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Register,
            new_consensus_key: Some(
                KeyDescriptor::from_public_key_bytes(KeyRole::ValidatorConsensus, PUBLIC_KEY)
                    .map_err(StateError::InvalidConsensusKey)?,
            ),
        })
    }

    fn activate() -> ValidatorUpdatePayloadV1 {
        ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Activate,
            new_consensus_key: None,
        }
    }

    #[test]
    fn encodes_register_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&register()?.encode()?),
            "01000101010020000000d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737"
        );
        Ok(())
    }

    #[test]
    fn encodes_activate_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(hex(&activate().encode()?), "01000200");
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        for payload in [register()?, activate()] {
            let decoded = ValidatorUpdatePayloadV1::decode(&payload.encode()?)?;
            assert_eq!(decoded, payload);
        }
        Ok(())
    }

    #[test]
    fn rejects_unsupported_payload_version() -> StateResult<()> {
        let mut encoded = activate().encode()?;
        encoded[0] = 0x02; // payload_version low byte, little-endian
        assert_eq!(
            ValidatorUpdatePayloadV1::decode(&encoded),
            Err(StateError::UnsupportedValidatorUpdatePayloadVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_operation() -> StateResult<()> {
        let mut encoded = activate().encode()?;
        encoded[2] = 0x09; // operation byte
        assert_eq!(
            ValidatorUpdatePayloadV1::decode(&encoded),
            Err(StateError::InvalidValidatorOperation { value: 0x09 })
        );
        Ok(())
    }

    #[test]
    fn rejects_a_key_present_on_an_operation_that_forbids_one() -> StateResult<()> {
        // activate()'s own encoding, but with the presence byte flipped
        // to true and a key appended -- structurally well-formed, but
        // activate must never carry a key.
        let mut encoded = activate().encode()?;
        let presence_index = encoded.len() - 1;
        encoded[presence_index] = 0x01;
        crate::validator_record::encode_key_descriptor(
            &mut encoded,
            &KeyDescriptor::from_public_key_bytes(KeyRole::ValidatorConsensus, PUBLIC_KEY)
                .map_err(StateError::InvalidConsensusKey)?,
        )
        .map_err(StateError::Encoding)?;

        assert_eq!(
            ValidatorUpdatePayloadV1::decode(&encoded),
            Err(StateError::ValidatorUpdateKeyPresenceMismatch)
        );
        Ok(())
    }

    #[test]
    fn rejects_a_missing_key_on_an_operation_that_requires_one() -> StateResult<()> {
        // register()'s own bytes, but with the presence byte flipped to
        // false and the key bytes dropped -- register must always carry
        // a key.
        let mut encoded = register()?.encode()?;
        let presence_index = 3; // payload_version(2) + operation(1)
        encoded[presence_index] = 0x00;
        encoded.truncate(presence_index + 1);

        assert_eq!(
            ValidatorUpdatePayloadV1::decode(&encoded),
            Err(StateError::ValidatorUpdateKeyPresenceMismatch)
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = activate().encode()?;
        encoded.push(0x00);
        assert!(matches!(
            ValidatorUpdatePayloadV1::decode(&encoded),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
