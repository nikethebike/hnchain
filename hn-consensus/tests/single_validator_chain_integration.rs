//! The first genuinely end-to-end proof of this whole pipeline: one
//! process acting as its own sole validator (100% of voting power, so
//! its own single vote is already a quorum) proposes, self-votes,
//! finalizes, and commits a real chain of several blocks in a row,
//! entirely on its own — no networking, no other participants, no
//! second node. This is deliberately the simplest possible case
//! (leader election is trivial with one candidate; quorum is trivial
//! with one signer) so it proves `propose -> apply -> commit -> next
//! block` genuinely works end-to-end before any of the real complexity
//! of a multi-validator network gets added on top.
//!
//! Three blocks, each with one real signed `transfer`, driven through
//! [`ConsensusEngine`] and committed to a real
//! [`hn_storage::InMemoryStateStore`]: block 2's transaction only
//! validates (correct nonce, no redundant bootstrap, sufficient
//! balance) because block 1's effects were genuinely committed to the
//! same backend first — proving cross-block state continuity through
//! the real `StateCommitter` path, not just a single height in
//! isolation the way `finalize_and_commit_integration.rs` already
//! covers.

use hn_consensus::{ConsensusAction, ConsensusEngine, ConsensusTarget};
use hn_core::{AccountNonce, BlockHeight, Epoch, Round};
use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
use hn_state::{
    AccessListV1, AccountSection, BalanceValueV1, BlockApplicationResult, QuorumCertificate,
    ReceiptStatus, StateCommitter, StateReader, StateWriter, TX_VERSION_1, TransactionEnvelope,
    TransferPayloadV1, TxType, ValidatorRecordV1, ValidatorSection, ValidatorStatus,
    ValidityWindowV1, VoteSigningPayloadV1, VoteTargetType, VoteType, account_section_state_key,
    tx_id, validator_section_state_key,
};
use hn_storage::InMemoryStateStore;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const NETWORK_ID: u16 = 1;
const VSC: [u8; 32] = [0x11; 32];
const SENDER_START_BALANCE: u128 = 1_000;

fn signed_transfer(
    keypair: &Ed25519KeyPair,
    sender: [u8; 32],
    recipient: [u8; 32],
    nonce: u64,
    bootstrap: bool,
    amount: u128,
) -> TestResult<TransactionEnvelope> {
    let payload = TransferPayloadV1 {
        recipient,
        asset_id: None,
        amount,
    }
    .encode()?;

    let mut envelope = TransactionEnvelope {
        tx_version: TX_VERSION_1,
        chain_id: 1,
        network_id: NETWORK_ID,
        tx_type: TxType::Transfer,
        sender,
        bootstrap_key: bootstrap.then(|| keypair.key_descriptor()),
        nonce: AccountNonce::new(nonce),
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

fn signed_qc(
    certificate_type: VoteType,
    validator_id: [u8; 32],
    keypair: &Ed25519KeyPair,
    height: BlockHeight,
    round: Round,
    block_hash: [u8; 32],
) -> TestResult<QuorumCertificate> {
    let payload = VoteSigningPayloadV1 {
        vote_type: certificate_type,
        chain_id: 1,
        network_id: 1,
        epoch: Epoch::new(0),
        height,
        round,
        validator_set_commitment: VSC,
        validator_id,
        target_type: VoteTargetType::Block,
        target_hash: block_hash,
        vote_metadata: Vec::new(),
    };
    let digest = payload.signing_digest()?;
    let signature = SignatureEnvelope {
        algorithm_id: keypair.key_descriptor().algorithm_id(),
        key_reference: None,
        signature: keypair.sign(&digest).to_vec(),
    };

    Ok(QuorumCertificate {
        certificate_type,
        chain_id: 1,
        network_id: 1,
        epoch: Epoch::new(0),
        height,
        round,
        validator_set_commitment: VSC,
        target_type: VoteTargetType::Block,
        target_hash: block_hash,
        total_voting_power: 100,
        signed_voting_power: 100,
        signer_commitment: vec![0b0000_0001],
        aggregate_proof: vec![signature],
    })
}

/// Drives one full height through `engine`, acting as the round's sole
/// proposer and sole voter — `propose -> self-prevote -> self-precommit
/// -> finalize -> commit -> next height`. This is what makes the
/// single-validator case trivial: one vote already carries all 100% of
/// voting power, so `verify_signatures` against a one-entry active set
/// is already a real, genuine quorum check, not a stub.
fn produce_block<S: StateReader + StateCommitter>(
    engine: &mut ConsensusEngine,
    store: &mut S,
    validator_id: [u8; 32],
    validator_keypair: &Ed25519KeyPair,
    block_hash: [u8; 32],
    transactions: Vec<TransactionEnvelope>,
) -> TestResult<BlockApplicationResult> {
    assert_eq!(engine.begin_round()?, ConsensusAction::None);

    let height = engine.state().height;
    let round = engine.state().round;
    let ordered_active_set = vec![validator_id];

    let action = engine.handle_proposal(block_hash, transactions, None)?;
    assert_eq!(
        action,
        ConsensusAction::Prevote(ConsensusTarget::Block(block_hash))
    );

    let prevote_qc = signed_qc(
        VoteType::Prevote,
        validator_id,
        validator_keypair,
        height,
        round,
        block_hash,
    )?;
    let action = engine.handle_quorum_certificate(prevote_qc, store, &ordered_active_set)?;
    assert_eq!(
        action,
        ConsensusAction::Precommit(ConsensusTarget::Block(block_hash))
    );

    let precommit_qc = signed_qc(
        VoteType::Precommit,
        validator_id,
        validator_keypair,
        height,
        round,
        block_hash,
    )?;
    let action = engine.handle_quorum_certificate(precommit_qc, store, &ordered_active_set)?;
    assert!(
        matches!(action, ConsensusAction::Finalized { block_hash: hash, .. } if hash == block_hash)
    );

    let result = engine.commit_finalized_block(block_hash, store, &[])?;

    let next_height = height.checked_next()?;
    assert_eq!(
        engine.begin_new_height()?,
        ConsensusAction::NewHeight {
            height: next_height
        }
    );

    Ok(result)
}

#[test]
fn single_validator_produces_a_chain_of_blocks_entirely_on_its_own() -> TestResult {
    let validator_keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x41; 32]);
    let validator_id = [0x01; 32];
    let sender_keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x42; 32]);
    let sender = account_address_body(
        NETWORK_ID,
        sender_keypair.key_descriptor().algorithm_id(),
        &sender_keypair.key_descriptor().public_key_bytes(),
    )?;
    let recipient = [0x55_u8; 32];

    let mut store = InMemoryStateStore::new();
    let record = ValidatorRecordV1 {
        validator_id,
        consensus_key: validator_keypair.key_descriptor(),
        bonded_stake: 100,
        voting_power: 100,
        status: ValidatorStatus::Active,
        pending_unbonding: None,
    };
    let record_key = validator_section_state_key(&validator_id, ValidatorSection::Record)?;
    store.set(record_key, record.encode()?)?;

    let balance_key = account_section_state_key(&sender, AccountSection::Balance)?;
    store.set(
        balance_key,
        BalanceValueV1 {
            native_balance: SENDER_START_BALANCE,
        }
        .encode(),
    )?;

    let mut engine = ConsensusEngine::new_height(BlockHeight::GENESIS);

    // Block 1 (height 0): bootstrap + transfer 100, nonce 0.
    let tx1 = signed_transfer(&sender_keypair, sender, recipient, 0, true, 100)?;
    let block1_hash = tx_id(&tx1.encode()?)?;
    let result1 = produce_block(
        &mut engine,
        &mut store,
        validator_id,
        &validator_keypair,
        block1_hash,
        vec![tx1],
    )?;
    assert_eq!(result1.applied.len(), 1);
    assert_eq!(result1.applied[0].receipt.status, ReceiptStatus::Success);
    assert_eq!(engine.state().height, BlockHeight::new(1));

    // Block 2 (height 1): a second, ordinary (non-bootstrap) transfer
    // from the same sender -- only valid if block 1's nonce advance and
    // Identity bootstrap were genuinely committed and are genuinely
    // visible now, through the same real backend.
    let tx2 = signed_transfer(&sender_keypair, sender, recipient, 1, false, 50)?;
    let block2_hash = tx_id(&tx2.encode()?)?;
    let result2 = produce_block(
        &mut engine,
        &mut store,
        validator_id,
        &validator_keypair,
        block2_hash,
        vec![tx2],
    )?;
    assert_eq!(result2.applied.len(), 1);
    assert_eq!(result2.applied[0].receipt.status, ReceiptStatus::Success);
    assert_eq!(engine.state().height, BlockHeight::new(2));

    // Block 3 (height 2): a third transfer, continuing the chain.
    let tx3 = signed_transfer(&sender_keypair, sender, recipient, 2, false, 25)?;
    let block3_hash = tx_id(&tx3.encode()?)?;
    let result3 = produce_block(
        &mut engine,
        &mut store,
        validator_id,
        &validator_keypair,
        block3_hash,
        vec![tx3],
    )?;
    assert_eq!(result3.applied.len(), 1);
    assert_eq!(result3.applied[0].receipt.status, ReceiptStatus::Success);
    assert_eq!(engine.state().height, BlockHeight::new(3));

    // Final state, read back through the real StateReader path,
    // reflecting all three blocks' cumulative effect: 1000 - 100 - 50 - 25.
    let sender_balance_key = account_section_state_key(&sender, AccountSection::Balance)?;
    let sender_balance = BalanceValueV1::decode(
        &store
            .get(&sender_balance_key)?
            .ok_or("sender balance missing after chain")?,
    )?;
    assert_eq!(sender_balance.native_balance, 1_000 - 100 - 50 - 25);

    let recipient_balance_key = account_section_state_key(&recipient, AccountSection::Balance)?;
    let recipient_balance = BalanceValueV1::decode(
        &store
            .get(&recipient_balance_key)?
            .ok_or("recipient balance missing after chain")?,
    )?;
    assert_eq!(recipient_balance.native_balance, 100 + 50 + 25);

    Ok(())
}
