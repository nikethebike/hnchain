//! End-to-end proof that `hn-crypto`'s Ed25519 identity, `hn-state`'s
//! validators-domain key derivation, `hn-smt-256-v1` state tree math,
//! `active_set`/`is_eligible_signer`, and `hn-list-merkle-v1`'s
//! `validators_root` commitment all compose on the same real data — the
//! validators-domain mirror of `account_integration.rs` (which is itself
//! key derivation plus in-memory tree math oracle-checked against an
//! independent Python implementation, not a real storage backend; this
//! test is the same kind of proof, not more).
//!
//! Every expected value is cross-checked against an independent Python
//! oracle (`scratchpad/validator_integration_oracle.py`), not derived
//! from this crate's own implementation.

use hn_crypto::{Ed25519KeyPair, KeyRole};
use hn_state::{
    EmptyHashTable, ValidatorRecordV1, ValidatorSection, ValidatorStatus, active_set,
    compute_state_root, is_eligible_signer, leaf_hash, list_merkle_root, validator_digest,
    validator_section_state_key, value_hash,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// `validator_id` here is an arbitrary placeholder, not a real derived
/// value: ADR-0012 decided `validator_id`'s width (`bytes32`) only, not
/// its exact derivation, which remains open.
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
        // Equal to voting_power here: this test exercises state-tree/
        // active-set composition, not bonded_stake's own independence
        // from voting_power (see validator_record.rs's unit tests and
        // ADR-0010, "Decided: `bonded_stake`, distinct from
        // `voting_power`" for that distinction).
        bonded_stake: voting_power,
        voting_power,
        status,
    }
}

fn four_validators() -> (
    ValidatorRecordV1,
    ValidatorRecordV1,
    ValidatorRecordV1,
    ValidatorRecordV1,
) {
    (
        validator([0x11; 32], [0xa1; 32], 300, ValidatorStatus::Active),
        validator([0x22; 32], [0xb1; 32], 100, ValidatorStatus::Active),
        validator([0x33; 32], [0xc1; 32], 200, ValidatorStatus::Active),
        validator([0x44; 32], [0xd1; 32], 500, ValidatorStatus::Jailed),
    )
}

#[test]
fn validator_records_derive_expected_state_keys_and_root() -> TestResult {
    let (a, b, c, d) = four_validators();

    let cases = [
        (
            &a,
            "a2c469748cb64985246fcb852591525092081a07381ab57ce2f9e37e5e310173",
        ),
        (
            &b,
            "afd24a06ff134294a9b7c278aad0ca09fb9330dfd11f6f274dc216b2d6df0401",
        ),
        (
            &c,
            "fc51fceed1456635539c0aa40977a42b33ed8371d28edcc61af0d7df09b4cbfb",
        ),
        (
            &d,
            "d3b94df17d55b638df0b8c2127a4dd00468ce795a84d2d0d195aed9b1ed78e4c",
        ),
    ];

    let mut leaves = Vec::new();
    for (record, expected_key) in cases {
        let key = validator_section_state_key(&record.validator_id, ValidatorSection::Record)?;
        assert_eq!(hex(&key), expected_key);
        let vh = value_hash(&record.encode()?)?;
        leaves.push((key, leaf_hash(&key, &vh)?));
    }

    let empty_table = EmptyHashTable::build()?;
    let root = compute_state_root(&leaves, &empty_table)?;
    assert_eq!(
        hex(&root),
        "13fd1da2fa6eddf6ff6641f448572c3b421cc5b7cc2d1ef7f4ab58691423d35a"
    );

    Ok(())
}

#[test]
fn active_set_and_jailing_overlay_compose_with_real_records() -> TestResult {
    let (a, b, c, d) = four_validators();

    // All four candidates participate in a real 4-way selection; B is
    // not individually asserted on beyond that (it simply doesn't make
    // the top 2 by voting_power).
    let candidates = vec![a.clone(), b, c.clone(), d];
    let selected = active_set(&candidates, 2);

    // D excluded despite the highest voting_power: Jailed, not Active.
    // Of the remaining Active validators, top 2 by voting_power are
    // A (300) and C (200); output is re-ordered ascending by
    // validator_id, which already matches selection order here.
    assert_eq!(
        selected.iter().map(|v| v.validator_id).collect::<Vec<_>>(),
        vec![a.validator_id, c.validator_id]
    );

    // A stayed Active through the epoch: eligible.
    assert!(is_eligible_signer(true, ValidatorStatus::Active));
    // D was never in the epoch's selected set at all: ineligible
    // regardless of its own status.
    assert!(!is_eligible_signer(false, ValidatorStatus::Jailed));
    // C *was* selected at the epoch snapshot but has since been jailed
    // mid-epoch (a live status change, not reflected in the frozen
    // snapshot) -- the live overlay excludes it immediately, exactly the
    // scenario ADR-0015's "Decided: jailing activation mechanism" exists
    // for.
    assert!(!is_eligible_signer(true, ValidatorStatus::Jailed));

    // validators_root over the selected set: validator_digest per
    // selected validator, then list_merkle_root over them in the same
    // ascending validator_id order active_set already returns.
    let mut digests = Vec::new();
    for record in &selected {
        digests.push(validator_digest(&record.encode()?)?);
    }
    let validators_root = list_merkle_root(&digests)?;
    assert_eq!(
        hex(&validators_root),
        "535e69dad146c5bbf7f9cf36777ea9db2eed03c866d775e7e8664a8cd4ecec3d"
    );

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
