//! Proof that `ConsensusEngine` really reaches into `hn-state` and a
//! real backend, not just its own pure transition table (ADR-0035,
//! "Wiring The Consensus Engine To `hn-state`"). Every unit test in
//! `hn-consensus`'s own `src/` uses a minimal in-memory `MapStore`;
//! this test runs the same sequence against `hn-storage::InMemoryStateStore`
//! — a real, if non-durable, `StateReader`/`StateWriter`/`StateCommitter`
//! implementation — and, critically, actually calls
//! `ConsensusEngine::commit_finalized_block`, which is
//! `apply_and_commit_block`'s own first real caller anywhere in this
//! codebase outside `hn-storage`'s own tests.
//!
//! `block_hash` here is an arbitrary placeholder, not derived from any
//! real `BlockHeader` — there is no concrete `BlockHeader` type
//! anywhere in this codebase yet (ADR-0035's own "Explicitly Not
//! Resolved"). This test only proves the wiring from "a block hash
//! finalized" to "its cached transactions were really applied and
//! committed," not that `block_hash` is computed correctly — that is a
//! distinct, later piece of work.

use hn_consensus::{ConsensusAction, ConsensusEngine, ConsensusTarget};
use hn_core::{AccountNonce, BlockHeight, Epoch, Round};
use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
use hn_state::{
    AccessListV1, AccountSection, BalanceValueV1, QuorumCertificate, ReceiptStatus, StateReader,
    StateWriter, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType, ValidatorRecordV1,
    ValidatorSection, ValidatorStatus, ValidityWindowV1, VoteSigningPayloadV1, VoteTargetType,
    VoteType, account_section_state_key, validator_section_state_key,
};
use hn_storage::InMemoryStateStore;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const NETWORK_ID: u16 = 1;
const BLOCK_HASH: [u8; 32] = [0x99; 32];
const VSC: [u8; 32] = [0x11; 32];
const SENDER_START_BALANCE: u128 = 1_000;
const TRANSFER_AMOUNT: u128 = 300;

fn seed_validator(
    store: &mut impl StateWriter,
    validator_id: [u8; 32],
    keypair: &Ed25519KeyPair,
) -> TestResult {
    let record = ValidatorRecordV1 {
        validator_id,
        consensus_key: keypair.key_descriptor(),
        bonded_stake: 100,
        voting_power: 100,
        status: ValidatorStatus::Active,
        pending_unbonding: None,
    };
    let key = validator_section_state_key(&validator_id, ValidatorSection::Record)?;
    store.set(key, record.encode()?)?;
    Ok(())
}

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

fn signed_qc(
    certificate_type: VoteType,
    validator_id: [u8; 32],
    keypair: &Ed25519KeyPair,
    round: u64,
) -> TestResult<QuorumCertificate> {
    let payload = VoteSigningPayloadV1 {
        vote_type: certificate_type,
        chain_id: 1,
        network_id: 1,
        epoch: Epoch::new(0),
        height: BlockHeight::GENESIS,
        round: Round::new(round),
        validator_set_commitment: VSC,
        validator_id,
        target_type: VoteTargetType::Block,
        target_hash: BLOCK_HASH,
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
        height: BlockHeight::GENESIS,
        round: Round::new(round),
        validator_set_commitment: VSC,
        target_type: VoteTargetType::Block,
        target_hash: BLOCK_HASH,
        total_voting_power: 100,
        signed_voting_power: 100,
        signer_commitment: vec![0b0000_0001],
        aggregate_proof: vec![signature],
    })
}

#[test]
fn engine_finalizes_and_really_commits_a_transfer_through_a_real_backend() -> TestResult {
    let validator_keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x21; 32]);
    let validator_id = [0x01; 32];
    let sender_keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x22; 32]);
    let sender = account_address_body(
        NETWORK_ID,
        sender_keypair.key_descriptor().algorithm_id(),
        &sender_keypair.key_descriptor().public_key_bytes(),
    )?;
    let recipient = [0x33_u8; 32];

    let mut store = InMemoryStateStore::new();
    seed_validator(&mut store, validator_id, &validator_keypair)?;
    let balance_key = account_section_state_key(&sender, AccountSection::Balance)?;
    store.set(
        balance_key,
        BalanceValueV1 {
            native_balance: SENDER_START_BALANCE,
        }
        .encode(),
    )?;

    let envelope = signed_bootstrap_transfer(&sender_keypair, recipient)?;
    let ordered_active_set = vec![validator_id];

    let mut engine = ConsensusEngine::new_height(BlockHeight::GENESIS);
    assert_eq!(engine.begin_round()?, ConsensusAction::None);

    let action = engine.handle_proposal(BLOCK_HASH, vec![envelope], None)?;
    assert_eq!(
        action,
        ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_HASH))
    );

    let prevote_qc = signed_qc(VoteType::Prevote, validator_id, &validator_keypair, 0)?;
    let action = engine.handle_quorum_certificate(prevote_qc, &store, &ordered_active_set)?;
    assert_eq!(
        action,
        ConsensusAction::Precommit(ConsensusTarget::Block(BLOCK_HASH))
    );
    assert_eq!(engine.state().locked_block, Some(BLOCK_HASH));

    let precommit_qc = signed_qc(VoteType::Precommit, validator_id, &validator_keypair, 0)?;
    let action = engine.handle_quorum_certificate(precommit_qc, &store, &ordered_active_set)?;
    assert!(
        matches!(action, ConsensusAction::Finalized { block_hash, .. } if block_hash == BLOCK_HASH)
    );

    // The real connection: commit_finalized_block -> apply_and_commit_block
    // -> a real StateCommitter -> the actual backend.
    let validator_records = vec![ValidatorRecordV1 {
        validator_id,
        consensus_key: validator_keypair.key_descriptor(),
        bonded_stake: 100,
        voting_power: 100,
        status: ValidatorStatus::Active,
        pending_unbonding: None,
    }];
    let result = engine.commit_finalized_block(BLOCK_HASH, &mut store, &validator_records)?;
    assert_eq!(result.applied.len(), 1);
    assert_eq!(result.applied[0].receipt.status, ReceiptStatus::Success);

    // Read every value back through the same real StateReader path --
    // not reused from `result`, not hand-computed.
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

    let action = engine.begin_new_height()?;
    assert_eq!(
        action,
        ConsensusAction::NewHeight {
            height: BlockHeight::new(1)
        }
    );
    assert_eq!(engine.state().locked_block, None);

    Ok(())
}
