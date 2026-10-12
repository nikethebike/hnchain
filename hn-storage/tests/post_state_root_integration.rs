//! `compute_post_state_root` (ADR-0045, "Decided: `state_root`
//! Computation Strategy (Devnet)") against both real backends —
//! enumerating a backend's own complete committed leaf set
//! (`StateEnumerator`) and overlaying a block's write set on top of
//! it, exactly as a validator would before ever committing anything.
//!
//! Every value below is cross-checked against an independent Python
//! oracle (`hashlib.sha512_256`, the same HNCS/`hn-smt-256-v1`
//! construction hand-replicated in Python) — not just self-consistent
//! with this crate's own code. The oracle's own low-level primitives
//! were themselves first validated against already-published Rust
//! test vectors (`hn-crypto`'s NIST SHA-512/256 vector, `hn-state`'s
//! own empty-root/`state_key_core` vectors) before computing anything
//! new, the same "verify, don't trust your own arithmetic" discipline
//! this project applies throughout.

use hn_core::{AccountNonce, BlockHeight};
use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
use hn_state::{
    AccessListV1, AccountSection, BalanceValueV1, EmptyHashTable, StateCommitter, StateEnumerator,
    StateReader, StateWriter, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType,
    ValidityWindowV1, account_section_state_key, apply_block, compute_post_state_root,
};
use hn_storage::{InMemoryStateStore, RedbStateStore};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const NETWORK_ID: u16 = 1;
const SENDER_SEED: [u8; 32] = [0xAA; 32];
const RECIPIENT: [u8; 32] = [0xBB; 32];
const INITIAL_BALANCE: u128 = 1_000_000;
const TRANSFER_AMOUNT: u128 = 100;

/// Independent-oracle values for this exact scenario (sender seed,
/// recipient, balance, amount, all above) — see the Python oracle this
/// pass's own "vector first, then code" step computed and verified
/// before any of this file was written.
const ORACLE_GENESIS_STATE_ROOT: &str =
    "eff7eca2409b8080b64419776ecac5427a2559dc938ec1d876befdfcf8640fce";
const ORACLE_POST_TRANSFER_STATE_ROOT: &str =
    "4cf63e0278ba3acd1202cae3ec07a5ab1f28b5f557d819ce3557db56f43994c2";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn signed_bootstrap_transfer(
    keypair: &Ed25519KeyPair,
    sender: [u8; 32],
) -> TestResult<TransactionEnvelope> {
    let payload = TransferPayloadV1 {
        recipient: RECIPIENT,
        asset_id: None,
        amount: TRANSFER_AMOUNT,
    }
    .encode()?;

    let mut envelope = TransactionEnvelope {
        tx_version: TX_VERSION_1,
        chain_id: 1,
        network_id: NETWORK_ID,
        tx_type: TxType::Transfer,
        sender,
        bootstrap_key: Some(keypair.key_descriptor()),
        nonce: AccountNonce::new(0),
        fee_limit: 100,
        validity_window: ValidityWindowV1 {
            min_height: None,
            max_height: None,
        },
        access_list: AccessListV1 {
            reads: vec![],
            writes: vec![],
        },
        payload,
        signatures: vec![],
    };
    let digest = envelope.signing_payload().signing_digest()?;
    envelope.signatures = vec![SignatureEnvelope {
        algorithm_id: keypair.key_descriptor().algorithm_id(),
        key_reference: None,
        signature: keypair.sign(&digest).to_vec(),
    }];
    Ok(envelope)
}

fn run_against<S>(mut store: S) -> TestResult
where
    S: StateReader + StateWriter + StateCommitter + StateEnumerator,
{
    let empty_table = EmptyHashTable::build()?;
    let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, SENDER_SEED);
    let sender = account_address_body(
        NETWORK_ID,
        keypair.key_descriptor().algorithm_id(),
        &keypair.key_descriptor().public_key_bytes(),
    )?;

    // Genesis: one funded account, nothing else -- seeded directly,
    // the same out-of-band "initial funding" use of `StateWriter`
    // `block_commit_integration.rs` already established.
    let sender_balance_key = account_section_state_key(&sender, AccountSection::Balance)?;
    store.set(
        sender_balance_key,
        BalanceValueV1 {
            native_balance: INITIAL_BALANCE,
        }
        .encode(),
    )?;

    // The post-state root of genesis alone (no write set to overlay)
    // must match the pre-transfer leaf set exactly.
    let genesis_root = compute_post_state_root(&store, &[], &empty_table)?;
    assert_eq!(hex(&genesis_root), ORACLE_GENESIS_STATE_ROOT);

    // A validator's own real check, before committing anything:
    // execute the block (`apply_block`, not `apply_and_commit_block`)
    // and overlay its write set onto the *current* enumerated state --
    // exactly ADR-0045's own "Decided: Proposal Validation Executes
    // The Block" step 3.
    let envelope = signed_bootstrap_transfer(&keypair, sender)?;
    let result = apply_block(&[envelope], &store, BlockHeight::GENESIS, &[])?;
    let write_set: Vec<_> = result
        .applied
        .iter()
        .flat_map(|applied| applied.write_set.clone())
        .collect();

    let pre_commit_root = compute_post_state_root(&store, &write_set, &empty_table)?;
    assert_eq!(hex(&pre_commit_root), ORACLE_POST_TRANSFER_STATE_ROOT);

    // Now actually commit, and confirm the *post-commit* enumerated
    // state (with an empty overlay) lands on the exact same root --
    // overlaying a write set and actually committing it must agree.
    store.commit(&write_set)?;
    let post_commit_root = compute_post_state_root(&store, &[], &empty_table)?;
    assert_eq!(hex(&post_commit_root), ORACLE_POST_TRANSFER_STATE_ROOT);

    Ok(())
}

#[test]
fn in_memory_store_post_state_root_matches_independent_oracle() -> TestResult {
    run_against(InMemoryStateStore::new())
}

#[test]
fn redb_store_post_state_root_matches_independent_oracle() -> TestResult {
    let dir = tempfile::tempdir()?;
    run_against(RedbStateStore::open(dir.path().join("state.redb"))?)
}
