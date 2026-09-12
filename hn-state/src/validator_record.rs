use hn_core::BlockHeight;
use hn_crypto::{
    Digest, ED25519_ALGORITHM_ID, ED25519_PUBLIC_KEY_LEN, KeyDescriptor, KeyRole,
    PUBLIC_KEY_MAX_LEN,
};
use hn_hncs::{
    Decoder, HncsResult, write_bytes, write_fixed_bytes, write_optional, write_u8, write_u16,
    write_u64, write_u128,
};

use crate::error::{StateError, StateResult};

/// `record_version` for the current `ValidatorRecordV1` shape (ADR-0010,
/// "Validator Identity").
pub const RECORD_VERSION_1: u16 = 1;

/// The `status` registry (ADR-0010, "Validator Status"), closed for this
/// profile. `0x00` is reserved, matching every other closed registry in
/// this project (`account_type`, `lifecycle` state, `vote_type`, ...).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ValidatorStatus {
    /// A `ValidatorRecordV1` exists, but bonded stake has not yet met the
    /// (still open) minimum bond.
    Registered = 0x01,
    /// Bonded stake meets the minimum bond; awaiting the operator's
    /// explicit `validator_activate` (ADR-0010, "Decided: admission
    /// mechanism").
    Candidate = 0x02,
    /// Eligible to participate in consensus. Membership in a given
    /// epoch's actual selected set is a separate question — see
    /// [`crate::active_set`] (ADR-0010, "Decided: active set derivation
    /// mechanism").
    Active = 0x03,
    /// Not currently participating, by the validator's own choice
    /// (`validator_deactivate`) or after a jail period lapses.
    Inactive = 0x04,
    /// Excluded from signing and from `total_voting_power` immediately,
    /// as a live overlay on top of the epoch-frozen active set (ADR-0015,
    /// "Decided: jailing activation mechanism") — see
    /// [`crate::active_set::is_eligible_signer`].
    Jailed = 0x05,
    /// Permanently left the validator set.
    Exited = 0x06,
}

impl ValidatorStatus {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::Registered),
            0x02 => Ok(Self::Candidate),
            0x03 => Ok(Self::Active),
            0x04 => Ok(Self::Inactive),
            0x05 => Ok(Self::Jailed),
            0x06 => Ok(Self::Exited),
            _ => Err(StateError::InvalidValidatorStatus { value }),
        }
    }

    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// A validator record (ADR-0010, "Validator Identity"). Deliberately
/// narrower than ADR-0010's full conceptual `ValidatorRecordV1`
/// (`account_address`, `network_key`, `activation_epoch`,
/// `deactivation_epoch`, `metadata_hash` are not represented here): those
/// fields either have no decision pinning down their exact interpretation
/// yet (`account_address` — which account, and why, is not resolved
/// anywhere), aren't needed by active set derivation or the jailing
/// overlay at all (`activation_epoch`/`deactivation_epoch` are bookkeeping
/// — eligibility is fully determined by `status` and `voting_power`,
/// ADR-0010's own decided `ACTIVE_SET` formula), or are blocked on a
/// still-deferred section (`metadata_hash`, no driver yet, same as
/// account-state.md's Metadata deferral), or belong to a layer this crate
/// does not implement (`network_key`, P2P).
///
/// `consensus_key` reuses [`hn_crypto::KeyDescriptor`] directly rather
/// than a raw public key field — it already validates canonical Ed25519
/// points and is exactly ADR-0002's own named `consensus_key` concept,
/// not a bespoke duplicate. `key_role` is not part of the wire encoding:
/// a `ValidatorRecordV1.consensus_key` is a [`KeyRole::ValidatorConsensus`]
/// key by construction — storing it again would duplicate what this
/// field's very presence already guarantees, the same redundancy class as
/// `protocol_name`/`checksum_profile`/`hash_profile`/`quorum_threshold`
/// found repeatedly elsewhere this session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatorRecordV1 {
    /// A stable protocol identifier, independent of `consensus_key`
    /// (ADR-0012, "Decided: `validator_id` width, not its exact
    /// derivation"; ADR-0010, "Decided: `validator_id` derivation" —
    /// the controlling account's own `address_body`).
    pub validator_id: Digest,
    /// The validator's `validator_consensus` signing key (ADR-0002).
    pub consensus_key: KeyDescriptor,
    /// Raw bonded stake (ADR-0010, "Decided: `bonded_stake`, distinct
    /// from `voting_power`") — mutated directly by `stake`/`unstake`
    /// (ADR-0006). Distinct from `voting_power`: the capping algorithm
    /// computes `voting_power` from the *entire* candidate set's
    /// `bonded_stake` total, so it cannot be recomputed synchronously
    /// inside a single validator's own stake change.
    pub bonded_stake: u128,
    /// Capped voting power (ADR-0010, "Decided: capped stake-weighted
    /// voting power" / "Decided: voting power integer type — `u128`").
    /// Changed only by the capping algorithm's own full-candidate-set
    /// recomputation, never directly by a transaction.
    pub voting_power: u128,
    /// Lifecycle status (ADR-0010, "Validator Status").
    pub status: ValidatorStatus,
    /// A stake withdrawal in progress, if any (ADR-0010, "unbonding
    /// period"; amount decided ADR-0023, "Decided: Unbonding Period" —
    /// 21 days). `unstake` (`crate::apply_unstake`) sets this rather
    /// than crediting the account's balance immediately; `None` means
    /// no withdrawal is pending. At most one pending withdrawal per
    /// validator — a second `unstake` while one is already pending is
    /// rejected ([`StateError::PendingUnbondingAlreadyExists`]) rather
    /// than queued, the simplest correct behavior for a first
    /// implementation; a bounded queue of several simultaneous pending
    /// withdrawals is a natural, additive future generalization if
    /// needed; this schema does not have to change to support it.
    pub pending_unbonding: Option<PendingUnbondingV1>,
}

/// A stake withdrawal that has been requested (`unstake`) but has not
/// yet matured — see [`ValidatorRecordV1::pending_unbonding`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingUnbondingV1 {
    /// The amount being withdrawn, already debited from
    /// [`ValidatorRecordV1::bonded_stake`] at the moment `unstake` was
    /// applied.
    pub amount: u128,
    /// The height at which `amount` becomes credited to the account's
    /// spendable balance (`crate::apply_unbonding_release`).
    pub matures_at_height: BlockHeight,
}

/// Appends `pending`'s canonical encoding (`amount: u128` + `matures_at_height:
/// u64`) to `out`. `HncsResult`-typed like [`encode_key_descriptor`]: unlike
/// that function, decoding a [`PendingUnbondingV1`] has no possible domain
/// error either (every `u128`/`u64` bit pattern is a valid amount/height),
/// so both directions can be used directly inside `write_optional`/
/// `read_optional` (ADR-0004) — no hand-rolled presence flag needed, unlike
/// `ValidatorUpdatePayloadV1.new_consensus_key`'s own `KeyDescriptor` case.
fn encode_pending_unbonding(out: &mut Vec<u8>, pending: &PendingUnbondingV1) -> HncsResult<()> {
    write_u128(out, pending.amount);
    write_u64(out, pending.matures_at_height.get());
    Ok(())
}

/// Decodes a [`PendingUnbondingV1`] written by [`encode_pending_unbonding`].
fn decode_pending_unbonding(decoder: &mut Decoder<'_>) -> HncsResult<PendingUnbondingV1> {
    let amount = decoder.read_u128()?;
    let matures_at_height = BlockHeight::new(decoder.read_u64()?);
    Ok(PendingUnbondingV1 {
        amount,
        matures_at_height,
    })
}

/// Appends `key`'s canonical encoding (`algorithm_id: u16` + bounded
/// `public_key` bytes) to `out`. Shared by [`ValidatorRecordV1::encode`]
/// and `ValidatorUpdatePayloadV1::encode` (ADR-0006, "Decided:
/// `stake`/`unstake`/`validator_update` payload shapes") — both carry a
/// `KeyDescriptor` on the wire the same way, and duplicating this logic
/// risks the two silently drifting apart.
///
/// `HncsResult`-typed, not `StateResult`: encoding a `KeyDescriptor` can
/// only ever fail with a byte-framing error (the length field), never a
/// domain-specific one, so this can be called directly wherever a
/// `HncsResult`-typed closure is expected (unlike
/// [`decode_key_descriptor`], which cannot be — see
/// `ValidatorUpdatePayloadV1::decode`'s own documentation for why).
pub(crate) fn encode_key_descriptor(out: &mut Vec<u8>, key: &KeyDescriptor) -> HncsResult<()> {
    write_u16(out, key.algorithm_id());
    write_bytes(out, &key.public_key_bytes(), PUBLIC_KEY_MAX_LEN)
}

/// Decodes a [`KeyDescriptor`] written by [`encode_key_descriptor`],
/// always as a [`KeyRole::ValidatorConsensus`] key — the only role a
/// `consensus_key`/`new_consensus_key` field is ever used for, matching
/// why `key_role` itself is never stored (see
/// [`ValidatorRecordV1`]'s own documentation).
pub(crate) fn decode_key_descriptor(decoder: &mut Decoder<'_>) -> StateResult<KeyDescriptor> {
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
    KeyDescriptor::from_public_key_bytes(KeyRole::ValidatorConsensus, public_key)
        .map_err(StateError::InvalidConsensusKey)
}

impl ValidatorRecordV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, RECORD_VERSION_1);
        write_fixed_bytes(&mut out, &self.validator_id);
        encode_key_descriptor(&mut out, &self.consensus_key).map_err(StateError::Encoding)?;
        write_u128(&mut out, self.bonded_stake);
        write_u128(&mut out, self.voting_power);
        write_u8(&mut out, self.status.as_u8());
        write_optional(
            &mut out,
            self.pending_unbonding.as_ref(),
            encode_pending_unbonding,
        )
        .map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ValidatorRecordV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let record_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if record_version != RECORD_VERSION_1 {
            return Err(StateError::UnsupportedRecordVersion {
                value: record_version,
            });
        }

        let validator_id = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let consensus_key = decode_key_descriptor(&mut decoder)?;
        let bonded_stake = decoder.read_u128().map_err(StateError::Encoding)?;
        let voting_power = decoder.read_u128().map_err(StateError::Encoding)?;
        let status = ValidatorStatus::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let pending_unbonding = decoder
            .read_optional(decode_pending_unbonding)
            .map_err(StateError::Encoding)?;

        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            validator_id,
            consensus_key,
            bonded_stake,
            voting_power,
            status,
            pending_unbonding,
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_crypto::{ED25519_ALGORITHM_ID, KeyDescriptor, KeyRole};

    use super::{StateError, ValidatorRecordV1, ValidatorStatus};
    use crate::error::StateResult;

    const VALIDATOR_ID: [u8; 32] = [0x44; 32];

    // A canonical Ed25519 public key already verified in
    // hn_crypto::validator_address's own oracle-checked tests.
    const PUBLIC_KEY: [u8; 32] = [
        0xd0, 0x4a, 0xb2, 0x32, 0x74, 0x2b, 0xb4, 0xab, 0x3a, 0x13, 0x68, 0xbd, 0x46, 0x15, 0xe4,
        0xe6, 0xd0, 0x22, 0x4a, 0xb7, 0x1a, 0x01, 0x6b, 0xaf, 0x85, 0x20, 0xa3, 0x32, 0xc9, 0x77,
        0x87, 0x37,
    ];

    fn sample() -> StateResult<ValidatorRecordV1> {
        Ok(ValidatorRecordV1 {
            validator_id: VALIDATOR_ID,
            consensus_key: KeyDescriptor::from_public_key_bytes(
                KeyRole::ValidatorConsensus,
                PUBLIC_KEY,
            )
            .map_err(StateError::InvalidConsensusKey)?,
            bonded_stake: 2_000_000,
            voting_power: 1_000_000,
            status: ValidatorStatus::Active,
            pending_unbonding: None,
        })
    }

    #[test]
    fn encodes_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&sample()?.encode()?),
            "01004444444444444444444444444444444444444444444444444444444444444444010020000000d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c977873780841e0000000000000000000000000040420f000000000000000000000000000300"
        );
        Ok(())
    }

    #[test]
    fn encodes_with_pending_unbonding_matching_independent_oracle() -> StateResult<()> {
        let mut record = sample()?;
        record.pending_unbonding = Some(super::PendingUnbondingV1 {
            amount: 12_345,
            matures_at_height: hn_core::BlockHeight::new(500),
        });
        assert_eq!(
            hex(&record.encode()?),
            "01004444444444444444444444444444444444444444444444444444444444444444010020000000d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c977873780841e0000000000000000000000000040420f00000000000000000000000000030139300000000000000000000000000000f401000000000000"
        );
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let record = sample()?;
        let decoded = ValidatorRecordV1::decode(&record.encode()?)?;
        assert_eq!(decoded, record);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_record_version() -> StateResult<()> {
        let mut encoded = sample()?.encode()?;
        encoded[0] = 0x02; // record_version low byte, little-endian
        assert_eq!(
            ValidatorRecordV1::decode(&encoded),
            Err(StateError::UnsupportedRecordVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_unsupported_key_algorithm() -> StateResult<()> {
        let mut encoded = sample()?.encode()?;
        // algorithm_id is the u16 right after record_version + validator_id
        // (2 + 32 = byte offset 34), little-endian.
        assert_eq!(encoded[34], 0x01);
        assert_eq!(encoded[35], 0x00);
        encoded[34] = 0x02;
        assert_eq!(
            ValidatorRecordV1::decode(&encoded),
            Err(StateError::UnsupportedKeyAlgorithm { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_validator_status() -> StateResult<()> {
        let mut encoded = sample()?.encode()?;
        // status is the byte right before the trailing pending_unbonding
        // presence byte (sample()'s pending_unbonding is None, one byte).
        let status_index = encoded.len() - 2;
        assert_eq!(encoded[status_index], ValidatorStatus::Active.as_u8());
        encoded[status_index] = 0x07;
        assert_eq!(
            ValidatorRecordV1::decode(&encoded),
            Err(StateError::InvalidValidatorStatus { value: 0x07 })
        );
        Ok(())
    }

    #[test]
    fn algorithm_id_reflects_ed25519() -> StateResult<()> {
        assert_eq!(sample()?.consensus_key.algorithm_id(), ED25519_ALGORITHM_ID);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
