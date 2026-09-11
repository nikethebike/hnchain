//! Proof that a real `StateReader`/`StateWriter` store-then-query cycle
//! produces exactly the same results as `hn-state`'s own
//! `validator_integration.rs`, which hand-assembles the same four
//! validator records' leaves directly. Two independent code paths
//! (write-then-read-back-then-hash vs. hand-built) agreeing on the same
//! state root is a stronger correctness proof than either alone — and is
//! the first real exercise of `hn_state::active_key`/
//! `fetch_validator_record` against something other than a slice the
//! test itself assembled.
//!
//! Every assertion here runs against both `hn-storage` implementations
//! (`InMemoryStateStore` and, per ADR-0019's "Decided: initial storage
//! backend", `RedbStateStore`) via the same shared helpers, over `impl
//! StateReader`/`impl StateWriter` — not because the two backends are
//! expected to behave differently, but because that is the entire point
//! of ADR-0019's "Backend Independence" rule: protocol code (and this
//! test) should not need to know or care which one is underneath.
//!
//! The expected state root below is copied from `hn-state`'s own
//! `validator_integration.rs::validator_records_derive_expected_state_keys_and_root`,
//! not recomputed — the point is that this path reaches the *same*
//! already-oracle-verified value, not a fresh oracle run.

use std::{error::Error, fmt};

use hn_crypto::{Ed25519KeyPair, KeyRole};
use hn_state::{
    EmptyHashTable, StateReader, StateWriter, ValidatorRecordV1, ValidatorSection, ValidatorStatus,
    active_key, compute_state_root, fetch_validator_record, leaf_hash, validator_section_state_key,
    value_hash,
};
use hn_storage::{InMemoryStateStore, RedbStateStore};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Debug)]
struct MissingValue {
    message: String,
}

impl fmt::Display for MissingValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for MissingValue {}

fn missing(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(MissingValue {
        message: message.into(),
    })
}

/// Mirrors `hn-state/tests/validator_integration.rs`'s own `validator`
/// helper exactly, same seeds/ids/values, so the two tests are directly
/// comparable.
fn validator(
    seed: [u8; 32],
    validator_id: [u8; 32],
    voting_power: u128,
    status: ValidatorStatus,
) -> ValidatorRecordV1 {
    let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, seed);
    ValidatorRecordV1 {
        validator_id,
        consensus_key: keypair.key_descriptor(),
        // Equal to voting_power, matching hn-state's own
        // validator_integration.rs — this test isn't about
        // bonded_stake's own distinction from voting_power.
        bonded_stake: voting_power,
        voting_power,
        status,
    }
}

fn four_validators() -> [ValidatorRecordV1; 4] {
    [
        validator([0x11; 32], [0xa1; 32], 300, ValidatorStatus::Active),
        validator([0x22; 32], [0xb1; 32], 100, ValidatorStatus::Active),
        validator([0x33; 32], [0xc1; 32], 200, ValidatorStatus::Active),
        validator([0x44; 32], [0xd1; 32], 500, ValidatorStatus::Jailed),
    ]
}

fn seed_store(store: &mut impl StateWriter, records: &[ValidatorRecordV1; 4]) -> TestResult {
    for record in records {
        let key = validator_section_state_key(&record.validator_id, ValidatorSection::Record)?;
        store.set(key, record.encode()?)?;
    }
    Ok(())
}

/// Reads every record's leaf back through `store` (rather than reusing
/// `records` directly, to actually exercise the `StateReader` path) and
/// asserts the resulting state root matches `hn-state`'s own
/// already-oracle-verified value.
fn assert_state_root_matches_hand_built(
    store: &impl StateReader,
    records: &[ValidatorRecordV1; 4],
) -> TestResult {
    let mut leaves = Vec::new();
    for record in records {
        let key = validator_section_state_key(&record.validator_id, ValidatorSection::Record)?;
        let bytes = store
            .get(&key)?
            .ok_or_else(|| missing("just written above"))?;
        let vh = value_hash(&bytes)?;
        leaves.push((key, leaf_hash(&key, &vh)?));
    }

    let empty_table = EmptyHashTable::build()?;
    let root = compute_state_root(&leaves, &empty_table)?;

    // Same value hn-state's own validator_integration.rs asserts,
    // reached via a completely different code path (store-backed, not
    // hand-assembled).
    assert_eq!(
        hex(&root),
        "13fd1da2fa6eddf6ff6641f448572c3b421cc5b7cc2d1ef7f4ab58691423d35a"
    );

    Ok(())
}

fn assert_active_key_and_fetch_validator_record_round_trip(
    store: &impl StateReader,
    records: &[ValidatorRecordV1; 4],
) -> TestResult {
    for record in records {
        let fetched = fetch_validator_record(store, &record.validator_id)?
            .ok_or_else(|| missing("was just written to the store above"))?;
        assert_eq!(&fetched, record);

        let key =
            active_key(store, &record.validator_id)?.ok_or_else(|| missing("record exists"))?;
        assert_eq!(
            key.public_key_bytes(),
            record.consensus_key.public_key_bytes()
        );
    }

    Ok(())
}

fn assert_fetch_and_active_key_return_none_for_an_unknown_validator(
    store: &impl StateReader,
) -> TestResult {
    let unknown_id = [0xff; 32];
    assert_eq!(fetch_validator_record(store, &unknown_id)?, None);
    assert_eq!(active_key(store, &unknown_id)?, None);
    Ok(())
}

#[test]
fn in_memory_store_round_trip_matches_hand_built_state_root() -> TestResult {
    let records = four_validators();
    let mut store = InMemoryStateStore::new();
    seed_store(&mut store, &records)?;
    assert_state_root_matches_hand_built(&store, &records)
}

#[test]
fn redb_store_round_trip_matches_hand_built_state_root() -> TestResult {
    let records = four_validators();
    let dir = tempfile::tempdir()?;
    let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
    seed_store(&mut store, &records)?;
    assert_state_root_matches_hand_built(&store, &records)
}

#[test]
fn in_memory_active_key_and_fetch_validator_record_round_trip_through_the_store() -> TestResult {
    let records = four_validators();
    let mut store = InMemoryStateStore::new();
    seed_store(&mut store, &records)?;
    assert_active_key_and_fetch_validator_record_round_trip(&store, &records)
}

#[test]
fn redb_active_key_and_fetch_validator_record_round_trip_through_the_store() -> TestResult {
    let records = four_validators();
    let dir = tempfile::tempdir()?;
    let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
    seed_store(&mut store, &records)?;
    assert_active_key_and_fetch_validator_record_round_trip(&store, &records)
}

#[test]
fn in_memory_fetch_and_active_key_return_none_for_an_unknown_validator() -> TestResult {
    let store = InMemoryStateStore::new();
    assert_fetch_and_active_key_return_none_for_an_unknown_validator(&store)
}

#[test]
fn redb_fetch_and_active_key_return_none_for_an_unknown_validator() -> TestResult {
    let dir = tempfile::tempdir()?;
    let store = RedbStateStore::open(dir.path().join("state.redb"))?;
    assert_fetch_and_active_key_return_none_for_an_unknown_validator(&store)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
