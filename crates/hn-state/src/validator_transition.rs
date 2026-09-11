use hn_crypto::Digest;

use crate::{
    error::{StateError, StateResult},
    node::{leaf_hash, value_hash},
    receipt::{ReceiptStatus, ReceiptV1},
    stake_payload::StakePayloadV1,
    tree::Leaf,
    unstake_payload::UnstakePayloadV1,
    validator::{ValidatorSection, validator_section_state_key},
    validator_record::{ValidatorRecordV1, ValidatorStatus},
    validator_update_payload::{ValidatorOperation, ValidatorUpdatePayloadV1},
};

/// Computes the one updated write-set leaf a `stake` produces (ADR-0006,
/// "Decided: `stake`/`unstake`/`validator_update` payload shapes"):
/// `record`'s `bonded_stake` increased by `payload.amount`.
///
/// `voting_power` is untouched — see [`ValidatorRecordV1`]'s own
/// documentation for why a single `stake` transaction cannot recompute
/// it: the capping algorithm needs the entire candidate set's total, not
/// one validator's own change.
///
/// Checks the validation precondition that `bonded_stake + amount` does
/// not overflow `u128` ([`StateError::BondedStakeOverflow`] —
/// practically unreachable given realistic amounts, but not silently
/// wrapped, mirroring [`crate::apply_transfer`]'s own `BalanceOverflow`
/// check).
pub fn apply_stake(record: &ValidatorRecordV1, payload: &StakePayloadV1) -> StateResult<[Leaf; 1]> {
    let bonded_stake = record
        .bonded_stake
        .checked_add(payload.amount)
        .ok_or(StateError::BondedStakeOverflow)?;
    let updated = ValidatorRecordV1 {
        bonded_stake,
        ..record.clone()
    };
    Ok([validator_record_leaf(&updated)?])
}

/// Applies a `stake` and produces its [`ReceiptV1`] in one step, mirroring
/// [`crate::apply_transfer_with_receipt`]'s own shape. `tx_id` is the
/// caller-computed transaction ID this receipt belongs to.
///
/// [`StateError::BondedStakeOverflow`] is a legitimate transaction
/// outcome, not a bug: it becomes a `ReceiptStatus::Failed` receipt with
/// no leaves to write. Any other error is a real internal error and is
/// propagated.
pub fn apply_stake_with_receipt(
    record: &ValidatorRecordV1,
    payload: &StakePayloadV1,
    tx_id: Digest,
) -> StateResult<(Option<[Leaf; 1]>, ReceiptV1)> {
    match apply_stake(record, payload) {
        Ok(leaves) => Ok((
            Some(leaves),
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Success,
            },
        )),
        Err(StateError::BondedStakeOverflow) => Ok((
            None,
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Failed,
            },
        )),
        Err(other) => Err(other),
    }
}

/// Computes the one updated write-set leaf an `unstake` produces:
/// `record`'s `bonded_stake` decreased by `payload.amount`.
///
/// Checks the validation precondition that `bonded_stake` is at least
/// `payload.amount` ([`StateError::InsufficientBondedStake`]).
/// Deliberately does not implement the unbonding-period release
/// mechanism (ADR-0010, still open) — only the immediate `bonded_stake`
/// bookkeeping effect, matching ADR-0006's own decision.
pub fn apply_unstake(
    record: &ValidatorRecordV1,
    payload: &UnstakePayloadV1,
) -> StateResult<[Leaf; 1]> {
    let bonded_stake = record
        .bonded_stake
        .checked_sub(payload.amount)
        .ok_or(StateError::InsufficientBondedStake)?;
    let updated = ValidatorRecordV1 {
        bonded_stake,
        ..record.clone()
    };
    Ok([validator_record_leaf(&updated)?])
}

/// Applies an `unstake` and produces its [`ReceiptV1`] in one step. See
/// [`apply_stake_with_receipt`]'s own documentation for the shared
/// success/failure-receipt shape.
pub fn apply_unstake_with_receipt(
    record: &ValidatorRecordV1,
    payload: &UnstakePayloadV1,
    tx_id: Digest,
) -> StateResult<(Option<[Leaf; 1]>, ReceiptV1)> {
    match apply_unstake(record, payload) {
        Ok(leaves) => Ok((
            Some(leaves),
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Success,
            },
        )),
        Err(StateError::InsufficientBondedStake) => Ok((
            None,
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Failed,
            },
        )),
        Err(other) => Err(other),
    }
}

/// Computes the one updated write-set leaf a `validator_update` produces
/// (ADR-0006, "Decided: `stake`/`unstake`/`validator_update` payload
/// shapes"): the discriminated status/key transition `payload.operation`
/// names, applied to `sender`'s record.
///
/// `existing` is `sender`'s current `ValidatorRecordV1`, if any —
/// `None` for a `sender` with no record yet, the only state
/// `ValidatorOperation::Register` is valid from.
///
/// Status preconditions, each an `InvalidValidatorStatusTransition` on
/// failure ([`UnknownValidator`](StateError::UnknownValidator) if
/// `existing` is `None` where a record is required):
///
/// - `Register`: `existing` must be `None`
///   ([`ValidatorAlreadyRegistered`](StateError::ValidatorAlreadyRegistered)
///   otherwise). Creates a new record: `validator_id = sender`,
///   `bonded_stake = 0`, `voting_power = 0`, `status = Registered`.
/// - `Activate`: `Candidate` or `Inactive` → `Active` — the latter is
///   the jailing-release reactivation path (ADR-0015, "Decided: jailing
///   activation mechanism": "reuses the already-decided
///   `validator_activate` operation... from `inactive`").
/// - `Deactivate`: `Active` → `Inactive`.
/// - `Exit`: `Inactive` → `Exited` — the lifecycle diagram's only
///   `exited` edge (ADR-0010); a `Candidate`/`Registered` validator
///   canceling before ever activating is a real gap this decision does
///   not resolve.
/// - `UpdateKeys`: any status except `Exited` → same status, replaces
///   `consensus_key`. Exact activation-epoch/old-key-validity-window
///   mechanics stay owned by ADR-0010's "Key Rotation," not enforced
///   here.
///
/// `Register`/`UpdateKeys` also require `payload.new_consensus_key` to
/// be present
/// ([`ValidatorUpdateKeyPresenceMismatch`](StateError::ValidatorUpdateKeyPresenceMismatch)
/// otherwise) — a defensive check, since a payload that reached this
/// function via [`ValidatorUpdatePayloadV1::decode`] already guarantees
/// it, but this function accepts any `&ValidatorUpdatePayloadV1`, not
/// only decoded ones.
pub fn apply_validator_update(
    existing: Option<&ValidatorRecordV1>,
    sender: Digest,
    payload: &ValidatorUpdatePayloadV1,
) -> StateResult<[Leaf; 1]> {
    let updated = match payload.operation {
        ValidatorOperation::Register => {
            if existing.is_some() {
                return Err(StateError::ValidatorAlreadyRegistered);
            }
            let consensus_key = payload
                .new_consensus_key
                .ok_or(StateError::ValidatorUpdateKeyPresenceMismatch)?;
            ValidatorRecordV1 {
                validator_id: sender,
                consensus_key,
                bonded_stake: 0,
                voting_power: 0,
                status: ValidatorStatus::Registered,
            }
        }
        ValidatorOperation::Activate => {
            let record = require_record(existing, sender)?;
            require_status(
                record,
                &[ValidatorStatus::Candidate, ValidatorStatus::Inactive],
                payload.operation,
            )?;
            ValidatorRecordV1 {
                status: ValidatorStatus::Active,
                ..record.clone()
            }
        }
        ValidatorOperation::Deactivate => {
            let record = require_record(existing, sender)?;
            require_status(record, &[ValidatorStatus::Active], payload.operation)?;
            ValidatorRecordV1 {
                status: ValidatorStatus::Inactive,
                ..record.clone()
            }
        }
        ValidatorOperation::Exit => {
            let record = require_record(existing, sender)?;
            require_status(record, &[ValidatorStatus::Inactive], payload.operation)?;
            ValidatorRecordV1 {
                status: ValidatorStatus::Exited,
                ..record.clone()
            }
        }
        ValidatorOperation::UpdateKeys => {
            let record = require_record(existing, sender)?;
            require_status(
                record,
                &[
                    ValidatorStatus::Registered,
                    ValidatorStatus::Candidate,
                    ValidatorStatus::Active,
                    ValidatorStatus::Inactive,
                    ValidatorStatus::Jailed,
                ],
                payload.operation,
            )?;
            let consensus_key = payload
                .new_consensus_key
                .ok_or(StateError::ValidatorUpdateKeyPresenceMismatch)?;
            ValidatorRecordV1 {
                consensus_key,
                ..record.clone()
            }
        }
    };

    Ok([validator_record_leaf(&updated)?])
}

/// Applies a `validator_update` and produces its [`ReceiptV1`] in one
/// step. Every rejection [`apply_validator_update`] can produce for a
/// well-formed `payload` is a legitimate transaction outcome (the
/// sender attempted a transition their record's current state does not
/// allow), so all of them become a `ReceiptStatus::Failed` receipt with
/// no leaves to write; any other error propagates.
pub fn apply_validator_update_with_receipt(
    existing: Option<&ValidatorRecordV1>,
    sender: Digest,
    payload: &ValidatorUpdatePayloadV1,
    tx_id: Digest,
) -> StateResult<(Option<[Leaf; 1]>, ReceiptV1)> {
    match apply_validator_update(existing, sender, payload) {
        Ok(leaves) => Ok((
            Some(leaves),
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Success,
            },
        )),
        Err(
            StateError::ValidatorAlreadyRegistered
            | StateError::UnknownValidator { .. }
            | StateError::InvalidValidatorStatusTransition { .. }
            | StateError::ValidatorUpdateKeyPresenceMismatch,
        ) => Ok((
            None,
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Failed,
            },
        )),
        Err(other) => Err(other),
    }
}

fn require_record(
    existing: Option<&ValidatorRecordV1>,
    validator_id: Digest,
) -> StateResult<&ValidatorRecordV1> {
    existing.ok_or(StateError::UnknownValidator { validator_id })
}

fn require_status(
    record: &ValidatorRecordV1,
    allowed: &[ValidatorStatus],
    operation: ValidatorOperation,
) -> StateResult<()> {
    if allowed.contains(&record.status) {
        Ok(())
    } else {
        Err(StateError::InvalidValidatorStatusTransition {
            status: record.status.as_u8(),
            operation: operation.as_u8(),
        })
    }
}

fn validator_record_leaf(record: &ValidatorRecordV1) -> StateResult<Leaf> {
    let key = validator_section_state_key(&record.validator_id, ValidatorSection::Record)?;
    let value_bytes = record.encode()?;
    let vh = value_hash(&value_bytes)?;
    Ok((key, leaf_hash(&key, &vh)?))
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::{
        apply_stake, apply_stake_with_receipt, apply_unstake, apply_unstake_with_receipt,
        apply_validator_update, apply_validator_update_with_receipt,
    };
    use crate::{
        ReceiptStatus, StakePayloadV1, StateError, UnstakePayloadV1, ValidatorOperation,
        ValidatorRecordV1, ValidatorSection, ValidatorStatus, ValidatorUpdatePayloadV1,
        error::StateResult, validator_section_state_key,
    };

    const SENDER: [u8; 32] = [0x11; 32];
    const TX_ID: [u8; 32] = [0x99; 32];

    fn record(
        bonded_stake: u128,
        voting_power: u128,
        status: ValidatorStatus,
    ) -> ValidatorRecordV1 {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x01; 32]);
        ValidatorRecordV1 {
            validator_id: SENDER,
            consensus_key: keypair.key_descriptor(),
            bonded_stake,
            voting_power,
            status,
        }
    }

    #[test]
    fn stake_increases_bonded_stake_and_leaves_voting_power_untouched() -> StateResult<()> {
        let before = record(1_000, 700, ValidatorStatus::Active);
        let payload = StakePayloadV1 { amount: 300 };

        let [leaf] = apply_stake(&before, &payload)?;

        let expected_key = validator_section_state_key(&SENDER, ValidatorSection::Record)?;
        assert_eq!(leaf.0, expected_key);

        let after = ValidatorRecordV1 {
            bonded_stake: 1_300,
            ..before.clone()
        };
        assert_eq!(leaf.1, super::validator_record_leaf(&after)?.1);
        Ok(())
    }

    #[test]
    fn stake_rejects_overflow() {
        let before = record(u128::MAX, 0, ValidatorStatus::Active);
        let payload = StakePayloadV1 { amount: 1 };
        assert_eq!(
            apply_stake(&before, &payload),
            Err(StateError::BondedStakeOverflow)
        );
    }

    #[test]
    fn stake_with_receipt_yields_success() -> StateResult<()> {
        let before = record(1_000, 0, ValidatorStatus::Active);
        let payload = StakePayloadV1 { amount: 300 };
        let (leaves, receipt) = apply_stake_with_receipt(&before, &payload, TX_ID)?;
        assert!(leaves.is_some());
        assert_eq!(receipt.status, ReceiptStatus::Success);
        Ok(())
    }

    #[test]
    fn stake_with_receipt_yields_failed_on_overflow() -> StateResult<()> {
        let before = record(u128::MAX, 0, ValidatorStatus::Active);
        let payload = StakePayloadV1 { amount: 1 };
        let (leaves, receipt) = apply_stake_with_receipt(&before, &payload, TX_ID)?;
        assert!(leaves.is_none());
        assert_eq!(receipt.status, ReceiptStatus::Failed);
        Ok(())
    }

    #[test]
    fn unstake_decreases_bonded_stake() -> StateResult<()> {
        let before = record(1_000, 700, ValidatorStatus::Active);
        let payload = UnstakePayloadV1 { amount: 300 };

        let [leaf] = apply_unstake(&before, &payload)?;

        let after = ValidatorRecordV1 {
            bonded_stake: 700,
            ..before.clone()
        };
        assert_eq!(leaf.1, super::validator_record_leaf(&after)?.1);
        Ok(())
    }

    #[test]
    fn unstake_rejects_insufficient_bonded_stake() {
        let before = record(10, 0, ValidatorStatus::Active);
        let payload = UnstakePayloadV1 { amount: 11 };
        assert_eq!(
            apply_unstake(&before, &payload),
            Err(StateError::InsufficientBondedStake)
        );
    }

    #[test]
    fn unstake_with_receipt_yields_failed_on_insufficient_stake() -> StateResult<()> {
        let before = record(10, 0, ValidatorStatus::Active);
        let payload = UnstakePayloadV1 { amount: 11 };
        let (leaves, receipt) = apply_unstake_with_receipt(&before, &payload, TX_ID)?;
        assert!(leaves.is_none());
        assert_eq!(receipt.status, ReceiptStatus::Failed);
        Ok(())
    }

    fn new_key() -> StateResult<hn_crypto::KeyDescriptor> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x02; 32]);
        Ok(keypair.key_descriptor())
    }

    #[test]
    fn register_creates_a_new_record() -> StateResult<()> {
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Register,
            new_consensus_key: Some(new_key()?),
        };

        let [leaf] = apply_validator_update(None, SENDER, &payload)?;

        let expected = ValidatorRecordV1 {
            validator_id: SENDER,
            consensus_key: new_key()?,
            bonded_stake: 0,
            voting_power: 0,
            status: ValidatorStatus::Registered,
        };
        assert_eq!(leaf.1, super::validator_record_leaf(&expected)?.1);
        Ok(())
    }

    #[test]
    fn register_rejects_an_existing_record() -> StateResult<()> {
        let existing = record(0, 0, ValidatorStatus::Registered);
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Register,
            new_consensus_key: Some(new_key()?),
        };
        assert_eq!(
            apply_validator_update(Some(&existing), SENDER, &payload),
            Err(StateError::ValidatorAlreadyRegistered)
        );
        Ok(())
    }

    #[test]
    fn activate_succeeds_from_candidate_and_inactive() -> StateResult<()> {
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Activate,
            new_consensus_key: None,
        };
        for status in [ValidatorStatus::Candidate, ValidatorStatus::Inactive] {
            let existing = record(0, 0, status);
            let [leaf] = apply_validator_update(Some(&existing), SENDER, &payload)?;
            let expected = ValidatorRecordV1 {
                status: ValidatorStatus::Active,
                ..existing
            };
            assert_eq!(leaf.1, super::validator_record_leaf(&expected)?.1);
        }
        Ok(())
    }

    #[test]
    fn activate_rejects_wrong_status() {
        let existing = record(0, 0, ValidatorStatus::Active);
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Activate,
            new_consensus_key: None,
        };
        assert_eq!(
            apply_validator_update(Some(&existing), SENDER, &payload),
            Err(StateError::InvalidValidatorStatusTransition {
                status: ValidatorStatus::Active as u8,
                operation: ValidatorOperation::Activate as u8,
            })
        );
    }

    #[test]
    fn deactivate_succeeds_from_active() -> StateResult<()> {
        let existing = record(0, 0, ValidatorStatus::Active);
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Deactivate,
            new_consensus_key: None,
        };
        let [leaf] = apply_validator_update(Some(&existing), SENDER, &payload)?;
        let expected = ValidatorRecordV1 {
            status: ValidatorStatus::Inactive,
            ..existing
        };
        assert_eq!(leaf.1, super::validator_record_leaf(&expected)?.1);
        Ok(())
    }

    #[test]
    fn exit_succeeds_from_inactive_only() -> StateResult<()> {
        let existing = record(0, 0, ValidatorStatus::Inactive);
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Exit,
            new_consensus_key: None,
        };
        let [leaf] = apply_validator_update(Some(&existing), SENDER, &payload)?;
        let expected = ValidatorRecordV1 {
            status: ValidatorStatus::Exited,
            ..existing
        };
        assert_eq!(leaf.1, super::validator_record_leaf(&expected)?.1);

        let active = record(0, 0, ValidatorStatus::Active);
        assert_eq!(
            apply_validator_update(Some(&active), SENDER, &payload),
            Err(StateError::InvalidValidatorStatusTransition {
                status: ValidatorStatus::Active as u8,
                operation: ValidatorOperation::Exit as u8,
            })
        );
        Ok(())
    }

    #[test]
    fn update_keys_replaces_consensus_key_and_rejects_exited() -> StateResult<()> {
        let existing = record(500, 300, ValidatorStatus::Active);
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::UpdateKeys,
            new_consensus_key: Some(new_key()?),
        };
        let [leaf] = apply_validator_update(Some(&existing), SENDER, &payload)?;
        let expected = ValidatorRecordV1 {
            consensus_key: new_key()?,
            ..existing
        };
        assert_eq!(leaf.1, super::validator_record_leaf(&expected)?.1);

        let exited = record(0, 0, ValidatorStatus::Exited);
        assert_eq!(
            apply_validator_update(Some(&exited), SENDER, &payload),
            Err(StateError::InvalidValidatorStatusTransition {
                status: ValidatorStatus::Exited as u8,
                operation: ValidatorOperation::UpdateKeys as u8,
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_operations_on_an_unknown_validator() {
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Deactivate,
            new_consensus_key: None,
        };
        assert_eq!(
            apply_validator_update(None, SENDER, &payload),
            Err(StateError::UnknownValidator {
                validator_id: SENDER
            })
        );
    }

    #[test]
    fn validator_update_with_receipt_yields_failed_for_an_invalid_transition() -> StateResult<()> {
        let existing = record(0, 0, ValidatorStatus::Active);
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Activate,
            new_consensus_key: None,
        };
        let (leaves, receipt) =
            apply_validator_update_with_receipt(Some(&existing), SENDER, &payload, TX_ID)?;
        assert!(leaves.is_none());
        assert_eq!(receipt.status, ReceiptStatus::Failed);
        Ok(())
    }

    #[test]
    fn validator_update_with_receipt_yields_success_for_a_valid_transition() -> StateResult<()> {
        let existing = record(0, 0, ValidatorStatus::Active);
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Deactivate,
            new_consensus_key: None,
        };
        let (leaves, receipt) =
            apply_validator_update_with_receipt(Some(&existing), SENDER, &payload, TX_ID)?;
        assert!(leaves.is_some());
        assert_eq!(receipt.status, ReceiptStatus::Success);
        Ok(())
    }
}
