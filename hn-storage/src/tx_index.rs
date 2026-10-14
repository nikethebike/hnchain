use hn_core::BlockHeight;
use hn_crypto::Digest;
use hn_state::{FinalizedBlock, StateResult, tx_id};

/// Derives `block`'s own `tx_id -> (height, index)` entries, in
/// transaction order — the one real computation
/// [`hn_state::BlockStore::tx_location`]'s own documentation names as
/// derived data, shared here so [`crate::InMemoryStateStore`] and
/// [`crate::RedbStateStore`] compute it identically rather than each
/// re-deriving it independently (ADR-0046, "Decided: Transaction
/// Index").
pub(crate) fn tx_locations_for_block(
    block: &FinalizedBlock,
) -> StateResult<Vec<(Digest, BlockHeight, u32)>> {
    let height = block.envelope.header.height;
    block
        .envelope
        .body
        .transactions
        .iter()
        .enumerate()
        .map(|(index, transaction)| {
            let id = tx_id(&transaction.encode()?)?;
            Ok((id, height, index as u32))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use hn_core::{AccountNonce, BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
    use hn_state::{
        AccessListV1, BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader,
        FINALIZED_BLOCK_VERSION_1, FinalizedBlock, HEADER_VERSION_1, QuorumCertificate,
        StateResult, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType,
        ValidityWindowV1, VoteTargetType, VoteType, list_empty_root,
    };

    use super::tx_locations_for_block;

    fn signed_transfer(seed: u8, nonce: u64) -> StateResult<TransactionEnvelope> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32]);
        let sender = account_address_body(
            1,
            keypair.key_descriptor().algorithm_id(),
            &keypair.key_descriptor().public_key_bytes(),
        )?;
        let payload = TransferPayloadV1 {
            recipient: [0x99; 32],
            asset_id: None,
            amount: 1,
        }
        .encode()?;
        let mut envelope = TransactionEnvelope {
            tx_version: TX_VERSION_1,
            chain_id: 1,
            network_id: 1,
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

    fn finalized_block_with_transactions(
        height: u64,
        transactions: Vec<TransactionEnvelope>,
    ) -> StateResult<FinalizedBlock> {
        let body = BlockBody {
            body_version: BODY_VERSION_1,
            receipts: Vec::new(),
            evidence: Vec::new(),
            extra_data: Vec::new(),
            transactions,
        };
        let header = BlockHeader {
            header_version: HEADER_VERSION_1,
            chain_id: 1,
            network_id: 1,
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
            extra_data_hash: body.extra_data_hash()?,
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
            chain_id: 1,
            network_id: 1,
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
        Ok(FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope,
            commit_qc,
        })
    }

    #[test]
    fn derives_one_entry_per_transaction_in_order() -> StateResult<()> {
        let tx_a = signed_transfer(0x01, 0)?;
        let tx_b = signed_transfer(0x02, 0)?;
        let expected_a = hn_state::tx_id(&tx_a.encode()?)?;
        let expected_b = hn_state::tx_id(&tx_b.encode()?)?;
        let block = finalized_block_with_transactions(7, vec![tx_a, tx_b])?;

        let locations = tx_locations_for_block(&block)?;
        assert_eq!(
            locations,
            vec![
                (expected_a, BlockHeight::new(7), 0),
                (expected_b, BlockHeight::new(7), 1),
            ]
        );
        Ok(())
    }

    #[test]
    fn is_empty_for_a_block_with_no_transactions() -> StateResult<()> {
        let block = finalized_block_with_transactions(1, Vec::new())?;
        assert_eq!(tx_locations_for_block(&block)?, Vec::new());
        Ok(())
    }
}
