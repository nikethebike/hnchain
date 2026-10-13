use std::collections::BTreeMap;

use hn_core::BlockHeight;
use hn_crypto::Digest;
use hn_state::{
    BlockStore, FinalizedBlock, StateCommitter, StateEnumerator, StateReader, StateResult,
    StateWriter, Write,
};

/// An in-memory [`StateReader`]/[`StateWriter`] implementation.
///
/// See the crate-level documentation for what this is (a trait-boundary
/// proof) and is not (a durable backend — see [`crate::RedbStateStore`]
/// for that). Always returns `Ok`: a `BTreeMap` genuinely cannot fail the
/// way a real backend can, even though the trait signature itself is now
/// fallible to accommodate that other implementation.
#[derive(Debug, Default)]
pub struct InMemoryStateStore {
    values: BTreeMap<Digest, Vec<u8>>,
    blocks: BTreeMap<u64, FinalizedBlock>,
}

impl InMemoryStateStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl StateReader for InMemoryStateStore {
    fn get(&self, state_key: &Digest) -> StateResult<Option<Vec<u8>>> {
        Ok(self.values.get(state_key).cloned())
    }
}

impl StateWriter for InMemoryStateStore {
    fn set(&mut self, state_key: Digest, value: Vec<u8>) -> StateResult<()> {
        self.values.insert(state_key, value);
        Ok(())
    }
}

impl StateCommitter for InMemoryStateStore {
    /// A plain loop is genuinely atomic here, not just nominally: a
    /// `BTreeMap` insert cannot itself fail, so there is no
    /// partial-failure mode to guard against beyond a panic, which
    /// would abort the whole process regardless of how the loop is
    /// written (ADR-0033).
    fn commit(&mut self, writes: &[Write]) -> StateResult<()> {
        for write in writes {
            self.values.insert(write.state_key, write.value.clone());
        }
        Ok(())
    }
}

impl StateEnumerator for InMemoryStateStore {
    fn enumerate(&self) -> StateResult<Vec<(Digest, Vec<u8>)>> {
        Ok(self
            .values
            .iter()
            .map(|(key, value)| (*key, value.clone()))
            .collect())
    }
}

impl BlockStore for InMemoryStateStore {
    /// A plain pair of inserts is genuinely atomic here, not just
    /// nominally: a `BTreeMap` insert cannot itself fail, so there is
    /// no partial-failure mode to guard against beyond a panic, which
    /// would abort the whole process regardless of how this is written
    /// (same reasoning as [`StateCommitter::commit`]'s own impl here).
    fn commit_block(&mut self, writes: &[Write], block: &FinalizedBlock) -> StateResult<()> {
        for write in writes {
            self.values.insert(write.state_key, write.value.clone());
        }
        self.blocks
            .insert(block.envelope.header.height.get(), block.clone());
        Ok(())
    }

    fn finalized_block_range(
        &self,
        from_height: BlockHeight,
        max_count: u32,
    ) -> StateResult<Vec<FinalizedBlock>> {
        Ok(self
            .blocks
            .range(from_height.get().saturating_add(1)..)
            .take(max_count as usize)
            .map(|(_, block)| block.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::InMemoryStateStore;
    use hn_core::{BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};
    use hn_crypto::SignatureEnvelope;
    use hn_state::{
        BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader, BlockStore,
        FINALIZED_BLOCK_VERSION_1, FinalizedBlock, HEADER_VERSION_1, QuorumCertificate,
        StateCommitter, StateEnumerator, StateReader, StateResult, StateWriter, VoteTargetType,
        VoteType, Write, list_empty_root,
    };

    fn sample_finalized_block(height: u64) -> StateResult<FinalizedBlock> {
        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions: Vec::new(),
            receipts: Vec::new(),
            evidence: Vec::new(),
            extra_data: Vec::new(),
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
    fn round_trips_a_stored_value() -> StateResult<()> {
        let mut store = InMemoryStateStore::new();
        let key = [0x11; 32];
        store.set(key, b"hello".to_vec())?;
        assert_eq!(store.get(&key)?, Some(b"hello".to_vec()));
        Ok(())
    }

    #[test]
    fn missing_key_reads_as_none() -> StateResult<()> {
        let store = InMemoryStateStore::new();
        assert_eq!(store.get(&[0x22; 32])?, None);
        Ok(())
    }

    #[test]
    fn set_overwrites_the_previous_value() -> StateResult<()> {
        let mut store = InMemoryStateStore::new();
        let key = [0x33; 32];
        store.set(key, b"first".to_vec())?;
        store.set(key, b"second".to_vec())?;
        assert_eq!(store.get(&key)?, Some(b"second".to_vec()));
        Ok(())
    }

    #[test]
    fn commit_applies_every_write_in_the_set() -> StateResult<()> {
        let mut store = InMemoryStateStore::new();
        let writes = vec![
            Write {
                state_key: [0x55; 32],
                value: b"one".to_vec(),
            },
            Write {
                state_key: [0x66; 32],
                value: b"two".to_vec(),
            },
        ];
        store.commit(&writes)?;
        assert_eq!(store.get(&[0x55; 32])?, Some(b"one".to_vec()));
        assert_eq!(store.get(&[0x66; 32])?, Some(b"two".to_vec()));
        Ok(())
    }

    #[test]
    fn enumerate_returns_every_stored_pair() -> StateResult<()> {
        let mut store = InMemoryStateStore::new();
        store.set([0x11; 32], b"one".to_vec())?;
        store.set([0x22; 32], b"two".to_vec())?;

        let mut entries = store.enumerate()?;
        entries.sort();
        assert_eq!(
            entries,
            vec![([0x11; 32], b"one".to_vec()), ([0x22; 32], b"two".to_vec()),]
        );
        Ok(())
    }

    #[test]
    fn enumerate_is_empty_for_a_fresh_store() -> StateResult<()> {
        let store = InMemoryStateStore::new();
        assert_eq!(store.enumerate()?, Vec::new());
        Ok(())
    }

    #[test]
    fn commit_block_applies_writes_and_stores_the_block_together() -> StateResult<()> {
        let mut store = InMemoryStateStore::new();
        let block = sample_finalized_block(1)?;
        let writes = vec![Write {
            state_key: [0x77; 32],
            value: b"applied".to_vec(),
        }];
        store.commit_block(&writes, &block)?;

        assert_eq!(store.get(&[0x77; 32])?, Some(b"applied".to_vec()));
        assert_eq!(
            store.finalized_block_range(BlockHeight::new(0), 10)?,
            vec![block]
        );
        Ok(())
    }

    #[test]
    fn finalized_block_range_returns_an_ascending_contiguous_prefix() -> StateResult<()> {
        let mut store = InMemoryStateStore::new();
        for height in 1..=5 {
            store.commit_block(&[], &sample_finalized_block(height)?)?;
        }

        let page = store.finalized_block_range(BlockHeight::new(1), 2)?;
        assert_eq!(
            page.iter()
                .map(|block| block.envelope.header.height.get())
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        Ok(())
    }

    #[test]
    fn finalized_block_range_is_empty_past_the_last_stored_height() -> StateResult<()> {
        let mut store = InMemoryStateStore::new();
        store.commit_block(&[], &sample_finalized_block(1)?)?;
        assert_eq!(
            store.finalized_block_range(BlockHeight::new(5), 10)?,
            Vec::new()
        );
        Ok(())
    }
}
