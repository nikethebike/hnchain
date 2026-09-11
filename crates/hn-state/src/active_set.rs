use hn_crypto::{Digest, KeyDescriptor};

use crate::error::StateResult;
use crate::state_store::StateReader;
use crate::validator::{ValidatorSection, validator_section_state_key};
use crate::validator_record::{ValidatorRecordV1, ValidatorStatus};

/// Derives an epoch's active validator set from a full set of candidate
/// records (ADR-0010, "Decided: active set derivation mechanism").
///
/// `candidates` is whatever the caller already fetched from canonical
/// state as of the epoch-start height — this crate owns state
/// *interfaces*, not a concrete storage engine (see the crate-level
/// documentation), so it takes an already-fetched slice rather than
/// querying anything itself. `max_size` is `MAX_ACTIVE_SET_SIZE`
/// (ADR-0010), still an open economic parameter — deliberately a
/// parameter here, not a constant, so this function does not have to
/// change once that value is decided.
///
/// ```text
/// ACTIVE_SET(epoch) -> Vec<ValidatorRecordV1>
///   candidates = { v : v.status == active }
///   ranked = candidates sorted by (voting_power desc, validator_id asc)
///   selected = ranked.take(MAX_ACTIVE_SET_SIZE)
///   ACTIVE_SET = selected, re-ordered ascending by validator_id
/// ```
///
/// Only `status == Active` records are eligible — `registered`/
/// `candidate` have not been admitted, `inactive`/`jailed`/`exited` have
/// left or been suspended (ADR-0010, "Decided: active set derivation
/// mechanism"). A record already `jailed` as of the snapshot height is
/// excluded here, the same as any other non-`active` status; jailing that
/// happens *after* the snapshot is a separate, live concern — see
/// [`is_eligible_signer`].
///
/// Ranking (`voting_power` descending, ties by `validator_id` ascending)
/// and output order (`validator_id` ascending, unconditionally) are two
/// different orderings for two different purposes: ranking decides who
/// makes the cut, output order gives a canonical, content-independent
/// sequence downstream code (`validators_root`, `signer_commitment` bit
/// indices) can rely on without recomputing it from stake.
pub fn active_set(candidates: &[ValidatorRecordV1], max_size: usize) -> Vec<ValidatorRecordV1> {
    let mut ranked: Vec<&ValidatorRecordV1> = candidates
        .iter()
        .filter(|record| record.status == ValidatorStatus::Active)
        .collect();

    ranked.sort_by(|a, b| {
        b.voting_power
            .cmp(&a.voting_power)
            .then_with(|| a.validator_id.cmp(&b.validator_id))
    });
    ranked.truncate(max_size);
    ranked.sort_by_key(|record| record.validator_id);

    ranked.into_iter().cloned().collect()
}

/// Whether a validator may currently sign, combining epoch-frozen
/// membership with a live jailing overlay (ADR-0015, "Decided: jailing
/// activation mechanism").
///
/// `in_epoch_active_set` is membership in the height's epoch-snapshotted
/// [`active_set`] (equivalently: committed under that epoch's
/// `validators_root`, ADR-0010) — this does not change mid-epoch.
/// `current_status` is the validator's live status, read fresh at the
/// height being verified, not from the frozen snapshot. A validator can
/// be jailed *after* its epoch snapshot was taken; this function is what
/// lets that take effect immediately rather than waiting for the next
/// epoch boundary, exactly as decided. `current_status` values other
/// than `active`/`jailed` do not arise here in practice — voluntary
/// deactivation/exit are epoch-delayed (ADR-0010, "Decided: admission
/// mechanism"), so a validator that was `active` at snapshot time can
/// only have since become `jailed`, not `inactive`/`exited`, mid-epoch —
/// but this function checks `current_status` directly rather than
/// assuming that invariant, so it stays correct even if that changes.
pub fn is_eligible_signer(in_epoch_active_set: bool, current_status: ValidatorStatus) -> bool {
    in_epoch_active_set && current_status != ValidatorStatus::Jailed
}

/// Fetches and decodes `validator_id`'s current [`ValidatorRecordV1`]
/// from `reader`, or `None` if nothing is stored at its
/// [`crate::validator_section_state_key`].
///
/// This is the "State Access Interface" step ADR-0019's own boundary
/// diagram places between the execution engine and a [`StateReader`]
/// backend, specialized to the `validators` domain's one decided section
/// — the building block [`active_key`] (below) is a thin wrapper over.
pub fn fetch_validator_record(
    reader: &impl StateReader,
    validator_id: &Digest,
) -> StateResult<Option<ValidatorRecordV1>> {
    let key = validator_section_state_key(validator_id, ValidatorSection::Record)?;
    match reader.get(&key) {
        Some(bytes) => Ok(Some(ValidatorRecordV1::decode(&bytes)?)),
        None => Ok(None),
    }
}

/// The `active_key(identity, role, height)` lookup ADR-0002's "Decided:
/// `SignatureEnvelope` concrete field list" depends on
/// (`key_reference` is context-derived from identity/role/height rather
/// than a stored field — see that decision's own reasoning), narrowed to
/// this profile's one relevant role (`validator_consensus`, implicit —
/// every [`ValidatorRecordV1.consensus_key`](ValidatorRecordV1) already
/// is one, by construction) and to "whatever height `reader` reflects":
/// this function does no height-indexed lookup itself, since this crate
/// does not implement historical/archival state access (ADR-0019,
/// `ArchiveStore` — no consumer, not attempted) — which `reader` to pass
/// in for a given height is entirely the caller's concern.
///
/// Returns `Ok(None)` only when no record exists for `validator_id` —
/// a stored record whose `consensus_key` this implementation cannot
/// decode (for example an unsupported algorithm) is an `Err`, not a
/// silent `None`, since [`ValidatorRecordV1::decode`] already rejects
/// that at decode time.
pub fn active_key(
    reader: &impl StateReader,
    validator_id: &Digest,
) -> StateResult<Option<KeyDescriptor>> {
    Ok(fetch_validator_record(reader, validator_id)?.map(|record| record.consensus_key))
}

#[cfg(test)]
mod tests {
    use hn_crypto::{KeyDescriptor, KeyRole};

    use super::{active_set, is_eligible_signer};
    use crate::error::{StateError, StateResult};
    use crate::validator_record::{ValidatorRecordV1, ValidatorStatus};

    const PUBLIC_KEY: [u8; 32] = [
        0xd0, 0x4a, 0xb2, 0x32, 0x74, 0x2b, 0xb4, 0xab, 0x3a, 0x13, 0x68, 0xbd, 0x46, 0x15, 0xe4,
        0xe6, 0xd0, 0x22, 0x4a, 0xb7, 0x1a, 0x01, 0x6b, 0xaf, 0x85, 0x20, 0xa3, 0x32, 0xc9, 0x77,
        0x87, 0x37,
    ];

    fn validator(
        id_byte: u8,
        voting_power: u128,
        status: ValidatorStatus,
    ) -> StateResult<ValidatorRecordV1> {
        Ok(ValidatorRecordV1 {
            validator_id: [id_byte; 32],
            consensus_key: KeyDescriptor::from_public_key_bytes(
                KeyRole::ValidatorConsensus,
                PUBLIC_KEY,
            )
            .map_err(StateError::InvalidConsensusKey)?,
            // Irrelevant to what these tests exercise (ranking/
            // eligibility by voting_power/status) — set equal to
            // voting_power for a reasonable default.
            bonded_stake: voting_power,
            voting_power,
            status,
        })
    }

    #[test]
    fn selects_top_k_by_voting_power_descending() -> StateResult<()> {
        let candidates = vec![
            validator(0x01, 100, ValidatorStatus::Active)?,
            validator(0x02, 300, ValidatorStatus::Active)?,
            validator(0x03, 200, ValidatorStatus::Active)?,
        ];

        let selected = active_set(&candidates, 2);

        // Top 2 by voting_power are ids 0x02 (300) and 0x03 (200);
        // output re-ordered ascending by validator_id.
        assert_eq!(
            selected
                .iter()
                .map(|v| v.validator_id[0])
                .collect::<Vec<_>>(),
            vec![0x02, 0x03]
        );
        Ok(())
    }

    #[test]
    fn ties_break_by_ascending_validator_id() -> StateResult<()> {
        let candidates = vec![
            validator(0x03, 100, ValidatorStatus::Active)?,
            validator(0x01, 100, ValidatorStatus::Active)?,
            validator(0x02, 100, ValidatorStatus::Active)?,
        ];

        // All tied at voting_power 100; only 2 slots, so the ranking
        // tie-break (ascending validator_id) picks 0x01 and 0x02.
        let selected = active_set(&candidates, 2);

        assert_eq!(
            selected
                .iter()
                .map(|v| v.validator_id[0])
                .collect::<Vec<_>>(),
            vec![0x01, 0x02]
        );
        Ok(())
    }

    #[test]
    fn excludes_non_active_statuses() -> StateResult<()> {
        let candidates = vec![
            validator(0x01, 999, ValidatorStatus::Registered)?,
            validator(0x02, 999, ValidatorStatus::Candidate)?,
            validator(0x03, 999, ValidatorStatus::Active)?,
            validator(0x04, 999, ValidatorStatus::Inactive)?,
            validator(0x05, 999, ValidatorStatus::Jailed)?,
            validator(0x06, 999, ValidatorStatus::Exited)?,
        ];

        let selected = active_set(&candidates, 10);

        assert_eq!(
            selected
                .iter()
                .map(|v| v.validator_id[0])
                .collect::<Vec<_>>(),
            vec![0x03]
        );
        Ok(())
    }

    #[test]
    fn output_order_is_always_ascending_validator_id_regardless_of_ranking() -> StateResult<()> {
        let candidates = vec![
            validator(0x05, 500, ValidatorStatus::Active)?,
            validator(0x01, 100, ValidatorStatus::Active)?,
            validator(0x03, 300, ValidatorStatus::Active)?,
        ];

        let selected = active_set(&candidates, 10);

        assert_eq!(
            selected
                .iter()
                .map(|v| v.validator_id[0])
                .collect::<Vec<_>>(),
            vec![0x01, 0x03, 0x05]
        );
        Ok(())
    }

    #[test]
    fn empty_candidates_yield_empty_set() {
        assert_eq!(active_set(&[], 10), Vec::new());
    }

    #[test]
    fn zero_max_size_yields_empty_set() -> StateResult<()> {
        let candidates = vec![validator(0x01, 100, ValidatorStatus::Active)?];
        assert_eq!(active_set(&candidates, 0), Vec::new());
        Ok(())
    }

    #[test]
    fn eligible_signer_requires_both_epoch_membership_and_not_jailed() {
        assert!(is_eligible_signer(true, ValidatorStatus::Active));
        assert!(!is_eligible_signer(true, ValidatorStatus::Jailed));
        assert!(!is_eligible_signer(false, ValidatorStatus::Active));
        assert!(!is_eligible_signer(false, ValidatorStatus::Jailed));
    }

    /// A minimal `StateReader` test double. `hn-state` cannot depend on
    /// `hn-storage`'s real `InMemoryStateStore` (the dependency direction
    /// runs the other way — see `state_store`'s module documentation), so
    /// this crate's own unit tests need their own tiny stand-in.
    struct MapReader(std::collections::BTreeMap<hn_crypto::Digest, Vec<u8>>);

    impl crate::state_store::StateReader for MapReader {
        fn get(&self, state_key: &hn_crypto::Digest) -> Option<Vec<u8>> {
            self.0.get(state_key).cloned()
        }
    }

    #[test]
    fn fetch_validator_record_round_trips_through_a_reader() -> StateResult<()> {
        use crate::validator::{ValidatorSection, validator_section_state_key};

        let record = validator(0x07, 250, ValidatorStatus::Active)?;
        let key = validator_section_state_key(&record.validator_id, ValidatorSection::Record)?;
        let reader = MapReader(std::collections::BTreeMap::from([(key, record.encode()?)]));

        let fetched = super::fetch_validator_record(&reader, &record.validator_id)?;
        assert_eq!(fetched, Some(record));

        Ok(())
    }

    #[test]
    fn active_key_extracts_the_consensus_key() -> StateResult<()> {
        use crate::validator::{ValidatorSection, validator_section_state_key};

        let record = validator(0x08, 250, ValidatorStatus::Active)?;
        let key = validator_section_state_key(&record.validator_id, ValidatorSection::Record)?;
        let reader = MapReader(std::collections::BTreeMap::from([(key, record.encode()?)]));

        let descriptor = super::active_key(&reader, &record.validator_id)?;
        assert_eq!(descriptor, Some(record.consensus_key));

        Ok(())
    }

    #[test]
    fn fetch_and_active_key_return_none_for_an_unknown_validator_id() -> StateResult<()> {
        let reader = MapReader(std::collections::BTreeMap::new());
        let unknown_id = [0xee; 32];

        assert_eq!(super::fetch_validator_record(&reader, &unknown_id)?, None);
        assert_eq!(super::active_key(&reader, &unknown_id)?, None);

        Ok(())
    }
}
