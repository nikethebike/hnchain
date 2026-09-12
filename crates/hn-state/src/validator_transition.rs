use hn_core::BlockHeight;
use hn_crypto::Digest;

use crate::{
    error::{StateError, StateResult},
    node::{leaf_hash, value_hash},
    receipt::{ReceiptStatus, ReceiptV1},
    stake_payload::StakePayloadV1,
    transfer::balance_leaf,
    tree::Leaf,
    unstake_payload::UnstakePayloadV1,
    validator::{ValidatorSection, validator_section_state_key},
    validator_record::{PendingUnbondingV1, ValidatorRecordV1, ValidatorStatus},
    validator_update_payload::{ValidatorOperation, ValidatorUpdatePayloadV1},
};

/// The unbonding period (ADR-0023, "Decided: Unbonding Period" — 21
/// days) expressed in blocks, derived from ADR-0009's "Decided: Target
/// Block Time" (2 seconds): `21 * 24 * 60 * 60 / 2 = 907_200`. Height-
/// based rather than timestamp-based — `BlockHeader.timestamp`'s own
/// consensus semantics are still undecided (ADR-0008, "Timestamp":
/// "must be consensus-defined"), so a consensus-critical maturity check
/// cannot rely on it yet; height is already the established choice for
/// an analogous "how long is this valid" question
/// ([`crate::ValidityWindowV1`], ADR-0006). This is an approximation of
/// 21 real-world days, not a guarantee: Tendermint-style block
/// production (ADR-0009) has no hard minimum-block-interval rule, so
/// actual elapsed time for 907,200 blocks can differ from 21 days if
/// real block production runs faster or slower than the 2-second
/// target — a known, accepted imprecision, not an oversight.
pub const UNBONDING_PERIOD_BLOCKS: u64 = 907_200;

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
/// `record`'s `bonded_stake` decreased by `payload.amount`, and a
/// [`PendingUnbondingV1`] recorded maturing `UNBONDING_PERIOD_BLOCKS`
/// after `current_height` (ADR-0010's unbonding period; amount and
/// block-time conversion decided ADR-0023/ADR-0009) —
/// [`apply_unbonding_release`] is what later credits it back to the
/// account's spendable balance, not this function.
///
/// Checks two validation preconditions: `record.pending_unbonding` must
/// be `None` ([`StateError::PendingUnbondingAlreadyExists`] — at most
/// one withdrawal in flight per validator, see
/// [`ValidatorRecordV1::pending_unbonding`]'s own documentation for why
/// a queue is deliberately not attempted yet), and `bonded_stake` must
/// be at least `payload.amount`
/// ([`StateError::InsufficientBondedStake`]).
pub fn apply_unstake(
    record: &ValidatorRecordV1,
    payload: &UnstakePayloadV1,
    current_height: BlockHeight,
) -> StateResult<[Leaf; 1]> {
    if record.pending_unbonding.is_some() {
        return Err(StateError::PendingUnbondingAlreadyExists);
    }
    let bonded_stake = record
        .bonded_stake
        .checked_sub(payload.amount)
        .ok_or(StateError::InsufficientBondedStake)?;
    let matures_at_height = current_height
        .get()
        .checked_add(UNBONDING_PERIOD_BLOCKS)
        .ok_or(StateError::UnbondingMaturityHeightOverflow)?;
    let updated = ValidatorRecordV1 {
        bonded_stake,
        pending_unbonding: Some(PendingUnbondingV1 {
            amount: payload.amount,
            matures_at_height: BlockHeight::new(matures_at_height),
        }),
        ..record.clone()
    };
    Ok([validator_record_leaf(&updated)?])
}

/// Applies an `unstake` and produces its [`ReceiptV1`] in one step. See
/// [`apply_stake_with_receipt`]'s own documentation for the shared
/// success/failure-receipt shape. Both of [`apply_unstake`]'s own
/// validation-precondition failures
/// ([`StateError::PendingUnbondingAlreadyExists`],
/// [`StateError::InsufficientBondedStake`]) are legitimate transaction
/// outcomes; [`StateError::UnbondingMaturityHeightOverflow`] is treated
/// the same way `BalanceOverflow`/`BondedStakeOverflow` already are
/// elsewhere in this module — practically unreachable, but a Failed
/// receipt rather than a propagated internal error if it ever occurs.
pub fn apply_unstake_with_receipt(
    record: &ValidatorRecordV1,
    payload: &UnstakePayloadV1,
    current_height: BlockHeight,
    tx_id: Digest,
) -> StateResult<(Option<[Leaf; 1]>, ReceiptV1)> {
    match apply_unstake(record, payload, current_height) {
        Ok(leaves) => Ok((
            Some(leaves),
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Success,
            },
        )),
        Err(
            StateError::PendingUnbondingAlreadyExists
            | StateError::InsufficientBondedStake
            | StateError::UnbondingMaturityHeightOverflow,
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

/// Credits a matured [`ValidatorRecordV1::pending_unbonding`] withdrawal
/// back to the account's spendable native balance and clears it —
/// `apply_unstake`'s own counterpart, run once maturity is reached
/// rather than at `unstake` time itself.
///
/// Returns `Ok(None)`, not an error, whenever there is nothing to do:
/// no pending withdrawal at all, or one that has not yet reached
/// `matures_at_height`. This is deliberate, not a placeholder — the
/// intended caller is a periodic sweep over every validator with a
/// pending withdrawal (a per-block "process matured unbondings" step),
/// which is expected to find nothing due most of the time it runs; that
/// caller does not exist yet (`hn-consensus`/`hn-node` are still stubs,
/// ADR-0019's own "no block-processing pipeline in this codebase yet"
/// situation), so nothing currently invokes this function — it is the
/// state-transition primitive such a caller will use once one exists.
///
/// `record.validator_id` is `native_balance`'s own account address
/// (ADR-0010, "Decided: `validator_id` derivation" — the controlling
/// account's own `address_body`), so no separate address parameter is
/// needed. Checks that crediting `current_native_balance` does not
/// overflow `u128` ([`StateError::BalanceOverflow`], reusing
/// `apply_transfer`'s own variant — the same failure mode, same
/// meaning).
///
/// Unlike [`apply_stake`]/[`apply_unstake`], this has no `_with_receipt`
/// counterpart: a receipt is tied to a specific `tx_id` a user's
/// transaction produced, and this function is not triggered by one.
pub fn apply_unbonding_release(
    record: &ValidatorRecordV1,
    current_native_balance: u128,
    current_height: BlockHeight,
) -> StateResult<Option<[Leaf; 2]>> {
    let Some(pending) = record.pending_unbonding else {
        return Ok(None);
    };
    if current_height.get() < pending.matures_at_height.get() {
        return Ok(None);
    }

    let new_balance = current_native_balance
        .checked_add(pending.amount)
        .ok_or(StateError::BalanceOverflow)?;
    let updated_record = ValidatorRecordV1 {
        pending_unbonding: None,
        ..record.clone()
    };

    Ok(Some([
        validator_record_leaf(&updated_record)?,
        balance_leaf(&record.validator_id, new_balance)?,
    ]))
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
/// - `Activate`: `Candidate`, `Inactive`, or `Jailed` → `Active`. The
///   `Jailed` case is the jailing-release reactivation path (ADR-0015,
///   "Reactivation" — no jail duration or cooldown: `Jailed` reactivates
///   directly, with no mandatory intermediate `Inactive` step).
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
                pending_unbonding: None,
            }
        }
        ValidatorOperation::Activate => {
            let record = require_record(existing, sender)?;
            require_status(
                record,
                &[
                    ValidatorStatus::Candidate,
                    ValidatorStatus::Inactive,
                    ValidatorStatus::Jailed,
                ],
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
        apply_stake, apply_stake_with_receipt, apply_unbonding_release, apply_unstake,
        apply_unstake_with_receipt, apply_validator_update, apply_validator_update_with_receipt,
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
            pending_unbonding: None,
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
    fn unstake_decreases_bonded_stake_and_records_pending_unbonding() -> StateResult<()> {
        let before = record(1_000, 700, ValidatorStatus::Active);
        let payload = UnstakePayloadV1 { amount: 300 };
        let current_height = hn_core::BlockHeight::new(1_000);

        let [leaf] = apply_unstake(&before, &payload, current_height)?;

        let after = ValidatorRecordV1 {
            bonded_stake: 700,
            pending_unbonding: Some(super::PendingUnbondingV1 {
                amount: 300,
                matures_at_height: hn_core::BlockHeight::new(
                    1_000 + super::UNBONDING_PERIOD_BLOCKS,
                ),
            }),
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
            apply_unstake(&before, &payload, hn_core::BlockHeight::new(0)),
            Err(StateError::InsufficientBondedStake)
        );
    }

    #[test]
    fn unstake_rejects_a_second_unstake_while_one_is_pending() -> StateResult<()> {
        let mut before = record(1_000, 700, ValidatorStatus::Active);
        before.pending_unbonding = Some(super::PendingUnbondingV1 {
            amount: 100,
            matures_at_height: hn_core::BlockHeight::new(5_000),
        });
        let payload = UnstakePayloadV1 { amount: 50 };
        assert_eq!(
            apply_unstake(&before, &payload, hn_core::BlockHeight::new(0)),
            Err(StateError::PendingUnbondingAlreadyExists)
        );
        Ok(())
    }

    #[test]
    fn unstake_with_receipt_yields_failed_on_insufficient_stake() -> StateResult<()> {
        let before = record(10, 0, ValidatorStatus::Active);
        let payload = UnstakePayloadV1 { amount: 11 };
        let (leaves, receipt) =
            apply_unstake_with_receipt(&before, &payload, hn_core::BlockHeight::new(0), TX_ID)?;
        assert!(leaves.is_none());
        assert_eq!(receipt.status, ReceiptStatus::Failed);
        Ok(())
    }

    #[test]
    fn unbonding_release_is_a_no_op_when_nothing_is_pending() -> StateResult<()> {
        let before = record(1_000, 700, ValidatorStatus::Active);
        assert_eq!(
            apply_unbonding_release(&before, 0, hn_core::BlockHeight::new(0))?,
            None
        );
        Ok(())
    }

    #[test]
    fn unbonding_release_is_a_no_op_before_maturity() -> StateResult<()> {
        let mut before = record(700, 700, ValidatorStatus::Active);
        before.pending_unbonding = Some(super::PendingUnbondingV1 {
            amount: 300,
            matures_at_height: hn_core::BlockHeight::new(1_000),
        });
        assert_eq!(
            apply_unbonding_release(&before, 0, hn_core::BlockHeight::new(999))?,
            None
        );
        Ok(())
    }

    #[test]
    fn unbonding_release_credits_balance_and_clears_pending_at_maturity() -> StateResult<()> {
        let mut before = record(700, 700, ValidatorStatus::Active);
        before.pending_unbonding = Some(super::PendingUnbondingV1 {
            amount: 300,
            matures_at_height: hn_core::BlockHeight::new(1_000),
        });

        let released = apply_unbonding_release(&before, 50, hn_core::BlockHeight::new(1_000))?;

        let expected_record = ValidatorRecordV1 {
            pending_unbonding: None,
            ..before.clone()
        };
        let expected_record_leaf = super::validator_record_leaf(&expected_record)?;
        let expected_balance_leaf = crate::transfer::balance_leaf(&before.validator_id, 350)?;
        assert_eq!(
            released,
            Some([expected_record_leaf, expected_balance_leaf])
        );
        Ok(())
    }

    #[test]
    fn unbonding_release_rejects_balance_overflow() -> StateResult<()> {
        let mut before = record(0, 0, ValidatorStatus::Active);
        before.pending_unbonding = Some(super::PendingUnbondingV1 {
            amount: 1,
            matures_at_height: hn_core::BlockHeight::new(0),
        });
        assert_eq!(
            apply_unbonding_release(&before, u128::MAX, hn_core::BlockHeight::new(0)),
            Err(StateError::BalanceOverflow)
        );
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
            pending_unbonding: None,
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
    fn activate_succeeds_from_candidate_inactive_and_jailed() -> StateResult<()> {
        let payload = ValidatorUpdatePayloadV1 {
            operation: ValidatorOperation::Activate,
            new_consensus_key: None,
        };
        for status in [
            ValidatorStatus::Candidate,
            ValidatorStatus::Inactive,
            ValidatorStatus::Jailed,
        ] {
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
