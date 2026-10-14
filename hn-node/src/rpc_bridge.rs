use hn_core::BlockHeight;
use hn_crypto::Digest;
use hn_rpc::{
    AccountReply, BlockHeaderReply, BlockReply, CommitQcSummary, MempoolReply, ReceiptEntry,
    RpcCall, RpcError, RpcReply, RpcResult, StatusReply, SubmittedReply, TransactionReply,
    TransactionStatus, ValidatorEntry, ValidatorStatusReply, ValidatorsReply,
};
use hn_state::{
    BlockStore, FinalizedBlock, ReceiptStatus, StateError, StateReader, StateResult,
    ValidatorRecordV1, ValidatorStatus, fetch_balance, fetch_identity, fetch_nonce,
};

use crate::error::{NodeError, NodeResult};
use crate::mempool::{MEMPOOL_MAX_TXS, Mempool, MempoolError};

/// Everything [`handle_rpc_call`] needs to answer one [`RpcCall`]
/// against this node's own current state (ADR-0046, "Decided:
/// Architecture And Crate Direction" — "Each call is O(1) or O(bounded
/// block size) work inside the loop"), taken as exactly these fields
/// rather than the whole node `Ctx` so this function stays testable
/// against a plain `InMemoryStateStore` without any P2P/consensus
/// machinery, the same "caller already resolved it" boundary
/// `verify_and_commit_synced_block`/`apply_block` already draw.
pub struct RpcContext<'a, S: StateReader + BlockStore> {
    /// This node's own durable store, read-only from this call's own
    /// point of view (a submit still only ever mutates `mempool`).
    pub store: &'a S,
    /// This node's own local transaction pool.
    pub mempool: &'a mut Mempool,
    /// HNChain protocol lineage.
    pub chain_id: u8,
    /// Network environment.
    pub network_id: u16,
    /// Genesis block's own `block_hash`.
    pub genesis_hash: Digest,
    /// This node's own protocol version, as `(major, minor, patch)`.
    pub protocol_version: (u16, u16, u16),
    /// This node's own last *finalized* height (distinct from the
    /// consensus engine's own currently-contested height).
    pub last_finalized_height: BlockHeight,
    /// That height's own block hash.
    pub last_block_hash: Digest,
    /// That height's own timestamp.
    pub last_timestamp: hn_core::UnixTimeMillis,
    /// The current validator candidate set, already resolved by the
    /// caller (same boundary `Mempool::admit`'s own parameter of the
    /// same name already draws).
    pub validator_candidates: &'a [ValidatorRecordV1],
    /// The number of currently-established peer connections.
    pub peer_count: usize,
    /// Whether this node is currently running ADR-0044's historical
    /// sync.
    pub syncing: bool,
}

/// Answers one [`RpcCall`] against `ctx` (ADR-0046, "Decided: v1
/// Endpoint Set"). The outer [`NodeResult`] is a genuine internal
/// failure (storage corruption, an index pointing at data that no
/// longer exists) — the same hard-error path every other store
/// operation in this crate already propagates via `?`, never
/// client-triggerable through a well-formed [`RpcCall`]. The inner
/// [`RpcResult`] is this ADR's own client-facing outcome: `Ok` with
/// the answer, or `Err` with one of its named stable error codes.
pub fn handle_rpc_call<S: StateReader + BlockStore>(
    call: RpcCall,
    ctx: &mut RpcContext<'_, S>,
) -> NodeResult<RpcResult> {
    match call {
        RpcCall::Status => Ok(Ok(RpcReply::Status(status_reply(ctx)))),
        RpcCall::LatestBlock => {
            let Some(block) = ctx.store.latest_finalized_block()? else {
                return Ok(Err(RpcError::BlockNotFound));
            };
            Ok(Ok(RpcReply::Block(Box::new(block_reply(&block)))))
        }
        RpcCall::BlockAtHeight { height } => {
            let Some(block) = fetch_block_at_height(ctx.store, height)? else {
                return Ok(Err(RpcError::BlockNotFound));
            };
            Ok(Ok(RpcReply::Block(Box::new(block_reply(&block)))))
        }
        RpcCall::Account { address } => {
            let exists = fetch_identity(ctx.store, &address)?.is_some();
            let balance = fetch_balance(ctx.store, &address)?.native_balance;
            let nonce = fetch_nonce(ctx.store, &address)?.get();
            Ok(Ok(RpcReply::Account(AccountReply {
                exists,
                balance,
                nonce,
                height: ctx.last_finalized_height.get(),
            })))
        }
        RpcCall::Transaction { tx_id } => transaction_reply(ctx, tx_id),
        RpcCall::SubmitTransaction { tx_bytes } => submit_transaction(ctx, tx_bytes),
        RpcCall::Mempool => Ok(Ok(RpcReply::Mempool(MempoolReply {
            size: ctx.mempool.len() as u64,
            capacity: MEMPOOL_MAX_TXS as u64,
        }))),
        RpcCall::Validators => Ok(Ok(RpcReply::Validators(ValidatorsReply {
            validators: ctx
                .validator_candidates
                .iter()
                .map(validator_entry)
                .collect(),
        }))),
    }
}

fn status_reply<S: StateReader + BlockStore>(ctx: &RpcContext<'_, S>) -> StatusReply {
    StatusReply {
        chain_id: ctx.chain_id,
        network_id: ctx.network_id,
        genesis_hash: ctx.genesis_hash,
        protocol_version: ctx.protocol_version,
        latest_height: ctx.last_finalized_height.get(),
        latest_block_hash: ctx.last_block_hash,
        latest_timestamp: ctx.last_timestamp.as_millis(),
        syncing: ctx.syncing,
        peer_count: ctx.peer_count as u64,
        mempool_size: ctx.mempool.len() as u64,
    }
}

/// Finds the finalized block at exactly `height`, or `None` — height 0
/// (genesis) is never stored as a [`FinalizedBlock`] (ADR-0043,
/// ADR-0044: genesis has no `commit_qc` of its own to pair with one),
/// so it is reported as not found here rather than synthesized from
/// the genesis manifest, which this function has no access to anyway.
fn fetch_block_at_height<S: BlockStore>(
    store: &S,
    height: BlockHeight,
) -> StateResult<Option<FinalizedBlock>> {
    let Some(previous) = height.get().checked_sub(1) else {
        return Ok(None);
    };
    let page = store.finalized_block_range(BlockHeight::new(previous), 1)?;
    Ok(page
        .into_iter()
        .find(|block| block.envelope.header.height == height))
}

fn block_reply(block: &FinalizedBlock) -> BlockReply {
    let header = &block.envelope.header;
    let receipts: Vec<ReceiptEntry> = block
        .envelope
        .body
        .receipts
        .iter()
        .map(|receipt| ReceiptEntry {
            tx_id: receipt.tx_id,
            success: matches!(receipt.status, ReceiptStatus::Success),
        })
        .collect();
    let tx_ids = receipts.iter().map(|receipt| receipt.tx_id).collect();
    BlockReply {
        header: BlockHeaderReply {
            header_version: header.header_version,
            chain_id: header.chain_id,
            network_id: header.network_id,
            height: header.height.get(),
            round: header.round.get(),
            epoch: header.epoch.get(),
            protocol_epoch: header.protocol_epoch.get(),
            parent_block_hash: header.parent_block_hash,
            proposer: header.proposer,
            timestamp: header.timestamp.as_millis(),
            transactions_root: header.transactions_root,
            state_root: header.state_root,
            receipts_root: header.receipts_root,
            events_root: header.events_root,
            consensus_root: header.consensus_root,
            evidence_root: header.evidence_root,
            protocol_parameters_hash: header.protocol_parameters_hash,
            extra_data_hash: header.extra_data_hash,
        },
        tx_ids,
        receipts,
        commit_qc: CommitQcSummary {
            height: block.commit_qc.height.get(),
            round: block.commit_qc.round.get(),
            signer_count: block.commit_qc.aggregate_proof.len(),
        },
    }
}

fn transaction_reply<S: StateReader + BlockStore>(
    ctx: &RpcContext<'_, S>,
    tx_id: Digest,
) -> NodeResult<RpcResult> {
    if let Some((height, index)) = ctx.store.tx_location(&tx_id)? {
        let block = fetch_block_at_height(ctx.store, height)?.ok_or_else(|| {
            NodeError::from(StateError::Storage(
                "tx_index points at a height with no finalized block".into(),
            ))
        })?;
        let receipt = block
            .envelope
            .body
            .receipts
            .get(index as usize)
            .ok_or_else(|| {
                NodeError::from(StateError::Storage(
                    "tx_index points past the end of its own block's receipts".into(),
                ))
            })?;
        return Ok(Ok(RpcReply::Transaction(TransactionReply {
            tx_id,
            status: TransactionStatus::Finalized {
                height: height.get(),
                index,
                success: matches!(receipt.status, ReceiptStatus::Success),
            },
        })));
    }
    if ctx.mempool.contains(&tx_id) {
        return Ok(Ok(RpcReply::Transaction(TransactionReply {
            tx_id,
            status: TransactionStatus::Pending,
        })));
    }
    Ok(Ok(RpcReply::Transaction(TransactionReply {
        tx_id,
        status: TransactionStatus::Unknown,
    })))
}

fn submit_transaction<S: StateReader + BlockStore>(
    ctx: &mut RpcContext<'_, S>,
    tx_bytes: Vec<u8>,
) -> NodeResult<RpcResult> {
    let outcome = ctx.mempool.admit(
        &tx_bytes,
        ctx.store,
        ctx.last_finalized_height,
        ctx.chain_id,
        ctx.network_id,
        ctx.validator_candidates,
    );
    Ok(match outcome {
        Ok(tx_id) => Ok(RpcReply::Submitted(SubmittedReply { tx_id })),
        Err(error) => Err(mempool_error_to_rpc_error(&error)),
    })
}

fn mempool_error_to_rpc_error(error: &MempoolError) -> RpcError {
    match error {
        MempoolError::Decode(_) => RpcError::TxDecodeFailed,
        MempoolError::WrongChainOrNetwork => RpcError::TxChainMismatch,
        MempoolError::Unverified(_) => RpcError::TxBadSignature,
        MempoolError::OutsideValidityWindow => RpcError::TxOutsideValidityWindow,
        MempoolError::ExecutionFailed(_) => RpcError::TxWouldFail,
        MempoolError::AlreadyPresent => RpcError::TxDuplicate,
        MempoolError::Full => RpcError::MempoolFull,
    }
}

fn validator_entry(record: &ValidatorRecordV1) -> ValidatorEntry {
    ValidatorEntry {
        address: record.validator_id,
        status: validator_status_to_reply(record.status),
        bonded_stake: record.bonded_stake,
    }
}

fn validator_status_to_reply(status: ValidatorStatus) -> ValidatorStatusReply {
    match status {
        ValidatorStatus::Registered => ValidatorStatusReply::Registered,
        ValidatorStatus::Candidate => ValidatorStatusReply::Candidate,
        ValidatorStatus::Active => ValidatorStatusReply::Active,
        ValidatorStatus::Inactive => ValidatorStatusReply::Inactive,
        ValidatorStatus::Jailed => ValidatorStatusReply::Jailed,
        ValidatorStatus::Exited => ValidatorStatusReply::Exited,
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{AccountNonce, Epoch, ProtocolEpoch, Round, UnixTimeMillis};
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
    use hn_state::{
        AccessListV1, BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader,
        FINALIZED_BLOCK_VERSION_1, HEADER_VERSION_1, QuorumCertificate, StateWriter, TX_VERSION_1,
        TransactionEnvelope, TransferPayloadV1, TxType, ValidatorStatus, ValidityWindowV1,
        VoteTargetType, VoteType, account_section_state_key, apply_block, list_empty_root,
    };
    use hn_storage::InMemoryStateStore;

    use super::*;

    const CHAIN_ID: u8 = 1;
    const NETWORK_ID: u16 = 1;

    fn funded_sender(
        store: &mut InMemoryStateStore,
        seed: u8,
        native_balance: u128,
    ) -> NodeResult<(Digest, Ed25519KeyPair)> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32]);
        let sender = account_address_body(
            NETWORK_ID,
            keypair.key_descriptor().algorithm_id(),
            &keypair.key_descriptor().public_key_bytes(),
        )?;
        let key = account_section_state_key(&sender, hn_state::AccountSection::Balance)?;
        store.set(key, hn_state::BalanceValueV1 { native_balance }.encode())?;
        Ok((sender, keypair))
    }

    fn signed_transfer(
        keypair: &Ed25519KeyPair,
        sender: Digest,
        nonce: u64,
        recipient: Digest,
        amount: u128,
    ) -> NodeResult<TransactionEnvelope> {
        let payload = TransferPayloadV1 {
            recipient,
            asset_id: None,
            amount,
        }
        .encode()?;
        let mut envelope = TransactionEnvelope {
            tx_version: TX_VERSION_1,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            tx_type: TxType::Transfer,
            sender,
            bootstrap_key: Some(keypair.key_descriptor()),
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

    /// Actually executes `transactions` against `store` (via
    /// `apply_block`) and commits both the resulting write-set and a
    /// real [`FinalizedBlock`] wrapping them together, through
    /// [`BlockStore::commit_block`] — mirrors the exact pattern
    /// `hn_storage`'s own `sample_finalized_block` test helpers use,
    /// with real transactions/receipts/write-effects rather than an
    /// empty body, so [`AccountReply`]/[`TransactionReply`] have real
    /// post-state to report on.
    fn commit_block_with_transactions(
        store: &mut InMemoryStateStore,
        height: u64,
        transactions: Vec<TransactionEnvelope>,
    ) -> NodeResult<FinalizedBlock> {
        let application = apply_block(&transactions, store, BlockHeight::new(height), &[])?;
        let receipts = application
            .applied
            .iter()
            .map(|applied| applied.receipt)
            .collect::<Vec<_>>();
        let write_set = application
            .applied
            .iter()
            .flat_map(|applied| applied.write_set.clone())
            .collect::<Vec<_>>();
        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions,
            receipts,
            evidence: Vec::new(),
            extra_data: Vec::new(),
        };
        let header = BlockHeader {
            header_version: HEADER_VERSION_1,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            height: BlockHeight::new(height),
            round: Round::new(0),
            epoch: Epoch::new(0),
            protocol_epoch: ProtocolEpoch::GENESIS,
            parent_block_hash: [0_u8; 32],
            proposer: [0x01_u8; 32],
            timestamp: UnixTimeMillis::from_millis(height),
            transactions_root: body.transactions_root()?,
            state_root: [0x02_u8; 32],
            receipts_root: body.receipts_root()?,
            events_root: list_empty_root()?,
            consensus_root: [0x03_u8; 32],
            evidence_root: body.evidence_root()?,
            protocol_parameters_hash: [0x04_u8; 32],
            extra_data_hash: hn_state::extra_data_hash(&[])?,
        };
        let envelope = BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification: None,
        };
        let block_hash = envelope.block_hash()?;
        let commit_qc = QuorumCertificate {
            certificate_type: VoteType::Precommit,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            epoch: Epoch::new(0),
            height: BlockHeight::new(height),
            round: Round::new(0),
            validator_set_commitment: [0x11_u8; 32],
            target_type: VoteTargetType::Block,
            target_hash: block_hash,
            total_voting_power: 100,
            signed_voting_power: 100,
            signer_commitment: vec![0b0000_0001],
            aggregate_proof: vec![SignatureEnvelope {
                algorithm_id: 1,
                key_reference: None,
                signature: vec![0xaa; 64],
            }],
        };
        let finalized = FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope,
            commit_qc,
        };
        store.commit_block(&write_set, &finalized)?;
        Ok(finalized)
    }

    fn base_context<'a>(
        store: &'a InMemoryStateStore,
        mempool: &'a mut Mempool,
        validator_candidates: &'a [ValidatorRecordV1],
    ) -> RpcContext<'a, InMemoryStateStore> {
        RpcContext {
            store,
            mempool,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            genesis_hash: [0x77; 32],
            protocol_version: (0, 1, 0),
            last_finalized_height: BlockHeight::new(0),
            last_block_hash: [0x77; 32],
            last_timestamp: UnixTimeMillis::from_millis(0),
            validator_candidates,
            peer_count: 2,
            syncing: false,
        }
    }

    #[test]
    fn status_reports_current_chain_state() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStateStore::new();
        let mut mempool = Mempool::new();
        let mut ctx = base_context(&store, &mut mempool, &[]);
        ctx.last_finalized_height = BlockHeight::new(3);
        ctx.syncing = true;

        let reply = handle_rpc_call(RpcCall::Status, &mut ctx)??;
        let RpcReply::Status(status) = reply else {
            return Err("expected RpcReply::Status".into());
        };
        assert_eq!(status.chain_id, CHAIN_ID);
        assert_eq!(status.network_id, NETWORK_ID);
        assert_eq!(status.latest_height, 3);
        assert!(status.syncing);
        assert_eq!(status.peer_count, 2);
        assert_eq!(status.mempool_size, 0);
        Ok(())
    }

    #[test]
    fn latest_block_is_not_found_against_an_empty_store() -> Result<(), Box<dyn std::error::Error>>
    {
        let store = InMemoryStateStore::new();
        let mut mempool = Mempool::new();
        let mut ctx = base_context(&store, &mut mempool, &[]);

        let reply = handle_rpc_call(RpcCall::LatestBlock, &mut ctx)?;
        assert_eq!(reply, Err(RpcError::BlockNotFound));
        Ok(())
    }

    #[test]
    fn latest_block_returns_the_most_recently_committed_block()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut store = InMemoryStateStore::new();
        commit_block_with_transactions(&mut store, 1, Vec::new())?;
        let block_2 = commit_block_with_transactions(&mut store, 2, Vec::new())?;
        let mut mempool = Mempool::new();
        let mut ctx = base_context(&store, &mut mempool, &[]);

        let reply = handle_rpc_call(RpcCall::LatestBlock, &mut ctx)??;
        let RpcReply::Block(block) = reply else {
            return Err("expected RpcReply::Block".into());
        };
        assert_eq!(block.header.height, 2);
        assert_eq!(
            block.commit_qc.signer_count,
            block_2.commit_qc.aggregate_proof.len()
        );
        Ok(())
    }

    #[test]
    fn block_at_height_finds_an_exact_height_and_rejects_one_above_latest()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut store = InMemoryStateStore::new();
        commit_block_with_transactions(&mut store, 1, Vec::new())?;
        commit_block_with_transactions(&mut store, 2, Vec::new())?;
        let mut mempool = Mempool::new();
        let mut ctx = base_context(&store, &mut mempool, &[]);

        let found = handle_rpc_call(
            RpcCall::BlockAtHeight {
                height: BlockHeight::new(1),
            },
            &mut ctx,
        )??;
        let RpcReply::Block(block) = found else {
            return Err("expected RpcReply::Block".into());
        };
        assert_eq!(block.header.height, 1);

        let missing = handle_rpc_call(
            RpcCall::BlockAtHeight {
                height: BlockHeight::new(5),
            },
            &mut ctx,
        )?;
        assert_eq!(missing, Err(RpcError::BlockNotFound));

        // Genesis (height 0) is never stored as a FinalizedBlock.
        let genesis = handle_rpc_call(
            RpcCall::BlockAtHeight {
                height: BlockHeight::GENESIS,
            },
            &mut ctx,
        )?;
        assert_eq!(genesis, Err(RpcError::BlockNotFound));
        Ok(())
    }

    #[test]
    fn account_reports_balance_nonce_and_existence() -> Result<(), Box<dyn std::error::Error>> {
        let mut store = InMemoryStateStore::new();
        let (sender, keypair) = funded_sender(&mut store, 0x01, 1_000)?;
        let recipient: Digest = [0x40; 32];
        let transfer = signed_transfer(&keypair, sender, 0, recipient, 10)?;
        commit_block_with_transactions(&mut store, 1, vec![transfer])?;
        let mut mempool = Mempool::new();
        let mut ctx = base_context(&store, &mut mempool, &[]);
        ctx.last_finalized_height = BlockHeight::new(1);

        let sender_reply = handle_rpc_call(RpcCall::Account { address: sender }, &mut ctx)??;
        let RpcReply::Account(account) = sender_reply else {
            return Err("expected RpcReply::Account".into());
        };
        assert!(account.exists);
        assert_eq!(account.balance, 990);
        assert_eq!(account.nonce, 1);
        assert_eq!(account.height, 1);

        let unknown_reply = handle_rpc_call(
            RpcCall::Account {
                address: [0x66; 32],
            },
            &mut ctx,
        )??;
        let RpcReply::Account(unknown) = unknown_reply else {
            return Err("expected RpcReply::Account".into());
        };
        assert!(!unknown.exists);
        assert_eq!(unknown.balance, 0);
        assert_eq!(unknown.nonce, 0);
        Ok(())
    }

    #[test]
    fn transaction_reports_finalized_pending_and_unknown() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut store = InMemoryStateStore::new();
        let (sender, keypair) = funded_sender(&mut store, 0x02, 1_000)?;
        let finalized_tx = signed_transfer(&keypair, sender, 0, [0x41; 32], 10)?;
        let finalized_tx_id = hn_state::tx_id(&finalized_tx.encode()?)?;
        commit_block_with_transactions(&mut store, 1, vec![finalized_tx])?;

        let (pending_sender, pending_keypair) = funded_sender(&mut store, 0x03, 1_000)?;
        let pending_tx = signed_transfer(&pending_keypair, pending_sender, 0, [0x42; 32], 5)?;
        let mut mempool = Mempool::new();
        let pending_tx_id = mempool.admit(
            &pending_tx.encode()?,
            &store,
            BlockHeight::new(1),
            CHAIN_ID,
            NETWORK_ID,
            &[],
        )?;
        let mut ctx = base_context(&store, &mut mempool, &[]);

        let finalized_reply = handle_rpc_call(
            RpcCall::Transaction {
                tx_id: finalized_tx_id,
            },
            &mut ctx,
        )??;
        let RpcReply::Transaction(reply) = finalized_reply else {
            return Err("expected RpcReply::Transaction".into());
        };
        assert_eq!(
            reply.status,
            TransactionStatus::Finalized {
                height: 1,
                index: 0,
                success: true,
            }
        );

        let pending_reply = handle_rpc_call(
            RpcCall::Transaction {
                tx_id: pending_tx_id,
            },
            &mut ctx,
        )??;
        let RpcReply::Transaction(reply) = pending_reply else {
            return Err("expected RpcReply::Transaction".into());
        };
        assert_eq!(reply.status, TransactionStatus::Pending);

        let unknown_reply = handle_rpc_call(RpcCall::Transaction { tx_id: [0x99; 32] }, &mut ctx)??;
        let RpcReply::Transaction(reply) = unknown_reply else {
            return Err("expected RpcReply::Transaction".into());
        };
        assert_eq!(reply.status, TransactionStatus::Unknown);
        Ok(())
    }

    #[test]
    fn submit_transaction_admits_a_valid_transaction() -> Result<(), Box<dyn std::error::Error>> {
        let mut store = InMemoryStateStore::new();
        let (sender, keypair) = funded_sender(&mut store, 0x04, 1_000)?;
        let transaction = signed_transfer(&keypair, sender, 0, [0x43; 32], 10)?;
        let expected_tx_id = hn_state::tx_id(&transaction.encode()?)?;
        let mut mempool = Mempool::new();
        let mut ctx = base_context(&store, &mut mempool, &[]);

        let reply = handle_rpc_call(
            RpcCall::SubmitTransaction {
                tx_bytes: transaction.encode()?,
            },
            &mut ctx,
        )??;
        let RpcReply::Submitted(submitted) = reply else {
            return Err("expected RpcReply::Submitted".into());
        };
        assert_eq!(submitted.tx_id, expected_tx_id);
        assert!(ctx.mempool.contains(&expected_tx_id));
        Ok(())
    }

    #[test]
    fn submit_transaction_maps_every_admission_failure_to_its_own_rpc_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut store = InMemoryStateStore::new();
        let (sender, keypair) = funded_sender(&mut store, 0x05, 1_000)?;
        let transaction = signed_transfer(&keypair, sender, 0, [0x44; 32], 10)?;
        let mut mempool = Mempool::new();
        let mut ctx = base_context(&store, &mut mempool, &[]);

        // A wrong chain_id on the submitted envelope itself exercises a
        // real admission rejection end to end, the same way an actually
        // malformed/unverifiable submission would.
        let mut wrong_chain = transaction.clone();
        wrong_chain.chain_id = CHAIN_ID + 1;
        let rejected = handle_rpc_call(
            RpcCall::SubmitTransaction {
                tx_bytes: wrong_chain.encode()?,
            },
            &mut ctx,
        )?;
        assert_eq!(rejected, Err(RpcError::TxChainMismatch));

        let oversized = vec![0_u8; hn_state::MAX_TRANSACTION_SIZE + 1];
        let decode_failed = handle_rpc_call(
            RpcCall::SubmitTransaction {
                tx_bytes: oversized,
            },
            &mut ctx,
        )?;
        assert_eq!(decode_failed, Err(RpcError::TxDecodeFailed));

        let admitted = handle_rpc_call(
            RpcCall::SubmitTransaction {
                tx_bytes: transaction.encode()?,
            },
            &mut ctx,
        )??;
        assert!(matches!(admitted, RpcReply::Submitted(_)));
        let duplicate = handle_rpc_call(
            RpcCall::SubmitTransaction {
                tx_bytes: transaction.encode()?,
            },
            &mut ctx,
        )?;
        assert_eq!(duplicate, Err(RpcError::TxDuplicate));
        Ok(())
    }

    #[test]
    fn mempool_reports_size_and_capacity() -> Result<(), Box<dyn std::error::Error>> {
        let mut store = InMemoryStateStore::new();
        let (sender, keypair) = funded_sender(&mut store, 0x06, 1_000)?;
        let transaction = signed_transfer(&keypair, sender, 0, [0x45; 32], 10)?;
        let mut mempool = Mempool::new();
        mempool.admit(
            &transaction.encode()?,
            &store,
            BlockHeight::new(0),
            CHAIN_ID,
            NETWORK_ID,
            &[],
        )?;
        let mut ctx = base_context(&store, &mut mempool, &[]);

        let reply = handle_rpc_call(RpcCall::Mempool, &mut ctx)??;
        let RpcReply::Mempool(mempool_reply) = reply else {
            return Err("expected RpcReply::Mempool".into());
        };
        assert_eq!(mempool_reply.size, 1);
        assert_eq!(mempool_reply.capacity, MEMPOOL_MAX_TXS as u64);
        Ok(())
    }

    #[test]
    fn validators_lists_every_candidate_record() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStateStore::new();
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x07; 32]);
        let validator_id: Digest = [0x50; 32];
        let record = ValidatorRecordV1 {
            validator_id,
            consensus_key: keypair.key_descriptor(),
            bonded_stake: 500,
            voting_power: 500,
            status: ValidatorStatus::Active,
            pending_unbonding: None,
        };
        let candidates = vec![record];
        let mut mempool = Mempool::new();
        let mut ctx = base_context(&store, &mut mempool, &candidates);

        let reply = handle_rpc_call(RpcCall::Validators, &mut ctx)??;
        let RpcReply::Validators(validators) = reply else {
            return Err("expected RpcReply::Validators".into());
        };
        assert_eq!(validators.validators.len(), 1);
        assert_eq!(validators.validators[0].address, validator_id);
        assert_eq!(validators.validators[0].bonded_stake, 500);
        assert_eq!(
            validators.validators[0].status,
            ValidatorStatusReply::Active
        );
        Ok(())
    }
}
