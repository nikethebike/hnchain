//! Proof that `StateWriter`/`StateCommitter` are finally reachable from
//! real protocol logic, not just test scaffolding (ADR-0033, "Atomic
//! Write-Set Commit"). Every other store-backed test in this crate
//! (`validator_store_integration.rs`) hand-seeds a store directly from
//! `ValidatorRecordV1.encode()` bytes it built itself; `apply_transaction`/
//! `apply_block` are never involved. This test is the first that is:
//!
//! build a real signed `TransactionEnvelope` -> `apply_and_commit_block`
//! (real business logic: signature verification, nonce/balance checks,
//! the transfer state transition) -> a real backend, committed through
//! `StateCommitter::commit` -> read back through `StateReader::get`.
//!
//! The sender's starting balance is seeded directly via `StateWriter::set`
//! first — a legitimate, separate use of it (out-of-band initial
//! funding, the same shape genesis allocation would need), not the gap
//! this ADR closes.

use hn_core::{AccountNonce, BlockHeight};
use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
use hn_state::{
    AccessListV1, AccountSection, BalanceValueV1, NonceValueV1, ReceiptStatus, StateReader,
    StateWriter, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType, ValidityWindowV1,
    account_section_state_key, apply_and_commit_block,
};
use hn_storage::{InMemoryStateStore, RedbStateStore};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const NETWORK_ID: u16 = 1;
const SENDER_START_BALANCE: u128 = 1_000;
const TRANSFER_AMOUNT: u128 = 300;

fn signed_bootstrap_transfer(
    keypair: &Ed25519KeyPair,
    recipient: [u8; 32],
) -> TestResult<TransactionEnvelope> {
    let sender = account_address_body(
        NETWORK_ID,
        keypair.key_descriptor().algorithm_id(),
        &keypair.key_descriptor().public_key_bytes(),
    )?;
    let payload = TransferPayloadV1 {
        recipient,
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

/// Seeds `sender`'s starting native balance directly via `StateWriter`
/// — the legitimate out-of-band use of it this ADR does not change
/// (genesis-style initial funding), distinct from `apply_and_commit_block`'s
/// own job of applying real transaction logic.
fn seed_balance(
    store: &mut impl StateWriter,
    account: &[u8; 32],
    native_balance: u128,
) -> TestResult {
    let key = account_section_state_key(account, AccountSection::Balance)?;
    store.set(key, BalanceValueV1 { native_balance }.encode())?;
    Ok(())
}

fn run_against<S: StateReader + StateWriter + hn_state::StateCommitter>(
    mut store: S,
) -> TestResult {
    let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x21; 32]);
    let sender = account_address_body(
        NETWORK_ID,
        keypair.key_descriptor().algorithm_id(),
        &keypair.key_descriptor().public_key_bytes(),
    )?;
    let recipient = [0x22_u8; 32];

    seed_balance(&mut store, &sender, SENDER_START_BALANCE)?;

    let envelope = signed_bootstrap_transfer(&keypair, recipient)?;

    let result = apply_and_commit_block(&[envelope], &mut store, BlockHeight::GENESIS, &[])?;

    assert_eq!(result.applied.len(), 1);
    assert_eq!(result.applied[0].receipt.status, ReceiptStatus::Success);

    // Every value below is read back through `StateReader::get` against
    // the real backend `apply_and_commit_block` just committed to — not
    // reused from `result` or hand-computed.
    let sender_balance_key = account_section_state_key(&sender, AccountSection::Balance)?;
    let sender_balance = BalanceValueV1::decode(
        &store
            .get(&sender_balance_key)?
            .ok_or("sender balance missing after commit")?,
    )?;
    assert_eq!(
        sender_balance.native_balance,
        SENDER_START_BALANCE - TRANSFER_AMOUNT
    );

    let recipient_balance_key = account_section_state_key(&recipient, AccountSection::Balance)?;
    let recipient_balance = BalanceValueV1::decode(
        &store
            .get(&recipient_balance_key)?
            .ok_or("recipient balance missing after commit")?,
    )?;
    assert_eq!(recipient_balance.native_balance, TRANSFER_AMOUNT);

    let sender_nonce_key = account_section_state_key(&sender, AccountSection::Nonce)?;
    let sender_nonce = NonceValueV1::decode(
        &store
            .get(&sender_nonce_key)?
            .ok_or("sender nonce missing after commit")?,
    )?;
    assert_eq!(sender_nonce.nonce, AccountNonce::new(1));

    // The bootstrap side effect: sender's IdentityValueV1 was also
    // written and committed, even though this test never called
    // `apply_identity_bootstrap` directly.
    let sender_identity_key = account_section_state_key(&sender, AccountSection::Identity)?;
    assert!(store.get(&sender_identity_key)?.is_some());

    Ok(())
}

#[test]
fn in_memory_store_commits_a_real_transfer_block() -> TestResult {
    run_against(InMemoryStateStore::new())
}

#[test]
fn redb_store_commits_a_real_transfer_block() -> TestResult {
    let dir = tempfile::tempdir()?;
    run_against(RedbStateStore::open(dir.path().join("state.redb"))?)
}
