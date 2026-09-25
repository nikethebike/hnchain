use hn_state::{
    StateReader, StateResult, StateWriter, ValidatorRecordV1, ValidatorSection, ValidatorStatus,
    validator_section_state_key,
};

use crate::identity::{consensus_keypair, validator_id};

/// Equal bonded stake/voting power every devnet validator record gets
/// (ADR-0037, "Decided: `hn-node` Process" — devnet genesis, not a real
/// economic parameter: this whole cluster exists only to prove
/// liveness/view-change, and 100 has no meaning beyond "every validator
/// carries the same weight").
const DEVNET_VOTING_POWER: u128 = 100;

/// Builds every validator's [`ValidatorRecordV1`] for a `validator_count`-
/// sized devnet cluster, all `Active` with equal voting power
/// (ADR-0037, "Decided: Devnet Validator Identity").
#[must_use]
pub fn devnet_validator_records(validator_count: u8) -> Vec<ValidatorRecordV1> {
    (0..validator_count)
        .map(|index| ValidatorRecordV1 {
            validator_id: validator_id(index),
            consensus_key: consensus_keypair(index).key_descriptor(),
            bonded_stake: DEVNET_VOTING_POWER,
            voting_power: DEVNET_VOTING_POWER,
            status: ValidatorStatus::Active,
            pending_unbonding: None,
        })
        .collect()
}

/// Writes every record in `records` to `store` if not already present —
/// idempotent (ADR-0037, "Decided: `hn-node` Process": "idempotent
/// devnet genesis"), so re-running against an existing `--data-dir`
/// does not overwrite whatever the store already has on file.
pub fn ensure_genesis_written<S: StateReader + StateWriter>(
    store: &mut S,
    records: &[ValidatorRecordV1],
) -> StateResult<()> {
    for record in records {
        let key = validator_section_state_key(&record.validator_id, ValidatorSection::Record)?;
        if store.get(&key)?.is_none() {
            store.set(key, record.encode()?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hn_storage::InMemoryStateStore;

    use super::{devnet_validator_records, ensure_genesis_written};
    use hn_state::{StateReader, StateResult, ValidatorSection, validator_section_state_key};

    #[test]
    fn writes_every_validator_record_once() -> StateResult<()> {
        let records = devnet_validator_records(4);
        assert_eq!(records.len(), 4);

        let mut store = InMemoryStateStore::new();
        ensure_genesis_written(&mut store, &records)?;
        for record in &records {
            let key = validator_section_state_key(&record.validator_id, ValidatorSection::Record)?;
            assert!(store.get(&key)?.is_some());
        }
        Ok(())
    }

    #[test]
    fn does_not_overwrite_an_existing_record() -> StateResult<()> {
        let records = devnet_validator_records(1);
        let mut store = InMemoryStateStore::new();
        ensure_genesis_written(&mut store, &records)?;

        let key = validator_section_state_key(&records[0].validator_id, ValidatorSection::Record)?;
        let first_write = store.get(&key)?;

        // Calling it again must be a no-op, not an error or a rewrite.
        ensure_genesis_written(&mut store, &records)?;
        assert_eq!(store.get(&key)?, first_write);
        Ok(())
    }
}
