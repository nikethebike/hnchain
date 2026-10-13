use std::path::Path;

use hn_core::BlockHeight;
use hn_crypto::Digest;
use hn_state::{
    BlockStore, FinalizedBlock, StateCommitter, StateEnumerator, StateError, StateReader,
    StateResult, StateWriter, Write,
};
use redb::{ReadableDatabase, ReadableTable, TableDefinition};

/// The one table this store keeps: `state_key -> canonical HNCS value
/// bytes`, exactly [`StateReader`]/[`StateWriter`]'s own scope. `&[u8]`
/// for both sides (not `[u8; 32]` for the key) — `redb`'s built-in `Key`
/// impl for `&[u8]` orders lexicographically by raw bytes, which is all
/// `StateReader`/`StateWriter` need; a fixed-width key type would gain
/// nothing here since every `state_key` this crate ever sees is already
/// exactly 32 bytes (ADR-0007).
const STATE_TABLE: TableDefinition<'_, &[u8], &[u8]> = TableDefinition::new("state");

/// `height -> canonical HNCS [`FinalizedBlock`] bytes` (ADR-0044,
/// "Decided: Durable Finalized-Block Storage") — a second table in the
/// same backend, not a new store with its own consistency story.
/// `u64`, not a fixed-width digest: this table is purely height-
/// ordered (ADR-0044, "Decided: Wire Protocol"), and `redb`'s built-in
/// `Key` impl for `u64` orders numerically, exactly what
/// [`BlockStore::finalized_block_range`] needs.
const BLOCKS_TABLE: TableDefinition<'_, u64, &[u8]> = TableDefinition::new("finalized_blocks");

/// A `redb`-backed durable [`StateReader`]/[`StateWriter`]/
/// [`StateCommitter`] implementation (ADR-0019, "Decided: initial
/// storage backend" — `redb`, a pure-Rust embedded ACID key-value store,
/// chosen over RocksDB specifically to avoid requiring a C++/cmake
/// toolchain for every contributor, and over deferring the choice since
/// ADR-0019's own phase ordering places storage ahead of node/RPC/CLI
/// work).
///
/// [`StateWriter::set`] goes through its own `redb` transaction per
/// call — a single-key-at-a-time operation, not a claim that several
/// `set` calls in a row commit atomically together.
/// [`StateCommitter::commit`] (ADR-0033, "Atomic Write-Set Commit") is
/// the real all-or-nothing path: one `redb` transaction for the whole
/// write set, relying on `redb`'s own transactional guarantee that an
/// uncommitted (errored or dropped) write transaction has no observable
/// effect at all. Still narrower than ADR-0019's full "Atomic State
/// Commit" scope (block header + state root + state tree nodes + ...
/// all consistent in one commit) — see `StateCommitter`'s own
/// documentation for exactly what remains out of scope.
pub struct RedbStateStore {
    database: redb::Database,
}

impl RedbStateStore {
    /// Opens (creating if absent) a `redb` database file at `path` and
    /// ensures [`STATE_TABLE`] exists, so a `get` against a
    /// freshly-created, never-written-to database returns `Ok(None)`
    /// rather than a `TableDoesNotExist` error — `redb`'s read-side
    /// `open_table` does not implicitly create a table the way its
    /// write-side one does.
    pub fn open(path: impl AsRef<Path>) -> StateResult<Self> {
        let database = redb::Database::create(path).map_err(storage_error)?;
        let write_txn = database.begin_write().map_err(storage_error)?;
        write_txn.open_table(STATE_TABLE).map_err(storage_error)?;
        write_txn.open_table(BLOCKS_TABLE).map_err(storage_error)?;
        write_txn.commit().map_err(storage_error)?;
        Ok(Self { database })
    }
}

impl StateReader for RedbStateStore {
    fn get(&self, state_key: &Digest) -> StateResult<Option<Vec<u8>>> {
        let read_txn = self.database.begin_read().map_err(storage_error)?;
        let table = read_txn.open_table(STATE_TABLE).map_err(storage_error)?;
        let stored = table.get(state_key.as_slice()).map_err(storage_error)?;
        Ok(stored.map(|guard| guard.value().to_vec()))
    }
}

impl StateWriter for RedbStateStore {
    fn set(&mut self, state_key: Digest, value: Vec<u8>) -> StateResult<()> {
        let write_txn = self.database.begin_write().map_err(storage_error)?;
        {
            let mut table = write_txn.open_table(STATE_TABLE).map_err(storage_error)?;
            table
                .insert(state_key.as_slice(), value.as_slice())
                .map_err(storage_error)?;
        }
        write_txn.commit().map_err(storage_error)?;
        Ok(())
    }
}

impl StateCommitter for RedbStateStore {
    /// One `redb` transaction for the whole slice: every entry is
    /// inserted into the same open `write_txn` before it commits once,
    /// so a failure anywhere in the loop (an `Err` returned before
    /// `commit()` is ever reached) leaves `write_txn` dropped without
    /// committing — `redb`'s own guarantee is that such a transaction
    /// has no effect on the database at all, matching
    /// [`StateCommitter`]'s all-or-nothing contract exactly.
    fn commit(&mut self, writes: &[Write]) -> StateResult<()> {
        let write_txn = self.database.begin_write().map_err(storage_error)?;
        {
            let mut table = write_txn.open_table(STATE_TABLE).map_err(storage_error)?;
            for write in writes {
                table
                    .insert(write.state_key.as_slice(), write.value.as_slice())
                    .map_err(storage_error)?;
            }
        }
        write_txn.commit().map_err(storage_error)?;
        Ok(())
    }
}

impl StateEnumerator for RedbStateStore {
    fn enumerate(&self) -> StateResult<Vec<(Digest, Vec<u8>)>> {
        let read_txn = self.database.begin_read().map_err(storage_error)?;
        let table = read_txn.open_table(STATE_TABLE).map_err(storage_error)?;
        table
            .iter()
            .map_err(storage_error)?
            .map(|entry| {
                let (key_guard, value_guard) = entry.map_err(storage_error)?;
                let state_key: Digest = key_guard
                    .value()
                    .try_into()
                    .map_err(|_| StateError::Storage("stored state_key is not 32 bytes".into()))?;
                Ok((state_key, value_guard.value().to_vec()))
            })
            .collect()
    }
}

impl BlockStore for RedbStateStore {
    /// One `redb` transaction across both tables: `writes` land in
    /// [`STATE_TABLE`] and `block` lands in [`BLOCKS_TABLE`] before the
    /// single `write_txn.commit()` call, so a failure anywhere before
    /// that point (an `Err` returned, or the process dying) leaves
    /// `write_txn` dropped without committing — `redb`'s own guarantee
    /// is that such a transaction has no effect on the database at
    /// all, exactly [`BlockStore`]'s own atomicity contract (ADR-0044,
    /// "Decided: Atomic Commit Of Block And State").
    fn commit_block(&mut self, writes: &[Write], block: &FinalizedBlock) -> StateResult<()> {
        let height = block.envelope.header.height.get();
        let encoded_block = block.encode()?;
        let write_txn = self.database.begin_write().map_err(storage_error)?;
        {
            let mut state_table = write_txn.open_table(STATE_TABLE).map_err(storage_error)?;
            for write in writes {
                state_table
                    .insert(write.state_key.as_slice(), write.value.as_slice())
                    .map_err(storage_error)?;
            }
            let mut blocks_table = write_txn.open_table(BLOCKS_TABLE).map_err(storage_error)?;
            blocks_table
                .insert(height, encoded_block.as_slice())
                .map_err(storage_error)?;
        }
        write_txn.commit().map_err(storage_error)?;
        Ok(())
    }

    fn finalized_block_range(
        &self,
        from_height: BlockHeight,
        max_count: u32,
    ) -> StateResult<Vec<FinalizedBlock>> {
        let read_txn = self.database.begin_read().map_err(storage_error)?;
        let table = read_txn.open_table(BLOCKS_TABLE).map_err(storage_error)?;
        table
            .range(from_height.get().saturating_add(1)..)
            .map_err(storage_error)?
            .take(max_count as usize)
            .map(|entry| {
                let (_, value_guard) = entry.map_err(storage_error)?;
                FinalizedBlock::decode(value_guard.value())
            })
            .collect()
    }

    fn latest_finalized_block(&self) -> StateResult<Option<FinalizedBlock>> {
        let read_txn = self.database.begin_read().map_err(storage_error)?;
        let table = read_txn.open_table(BLOCKS_TABLE).map_err(storage_error)?;
        table
            .last()
            .map_err(storage_error)?
            .map(|(_, value_guard)| FinalizedBlock::decode(value_guard.value()))
            .transpose()
    }
}

/// Converts any `redb` error into [`StateError::Storage`] via its own
/// `Display` output. Backend-agnostic by construction: this crate's
/// callers see only "a storage operation failed", never a `redb`-typed
/// error, matching ADR-0019's "Backend Independence" rule.
fn storage_error(error: impl Into<redb::Error>) -> StateError {
    StateError::Storage(error.into().to_string())
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};
    use hn_crypto::SignatureEnvelope;
    use hn_state::{
        BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader, BlockStore,
        FINALIZED_BLOCK_VERSION_1, FinalizedBlock, HEADER_VERSION_1, QuorumCertificate,
        StateCommitter, StateEnumerator, StateReader, StateResult, StateWriter, VoteTargetType,
        VoteType, Write, list_empty_root,
    };
    use tempfile::tempdir;

    use super::{BLOCKS_TABLE, STATE_TABLE, storage_error};

    use super::RedbStateStore;

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
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
        let key = [0x11; 32];
        store.set(key, b"hello".to_vec())?;
        assert_eq!(store.get(&key)?, Some(b"hello".to_vec()));
        Ok(())
    }

    #[test]
    fn missing_key_reads_as_none() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let store = RedbStateStore::open(dir.path().join("state.redb"))?;
        assert_eq!(store.get(&[0x22; 32])?, None);
        Ok(())
    }

    #[test]
    fn set_overwrites_the_previous_value() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
        let key = [0x33; 32];
        store.set(key, b"first".to_vec())?;
        store.set(key, b"second".to_vec())?;
        assert_eq!(store.get(&key)?, Some(b"second".to_vec()));
        Ok(())
    }

    #[test]
    fn a_value_survives_reopening_the_same_database_file() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let path = dir.path().join("state.redb");
        let key = [0x44; 32];

        let mut store = RedbStateStore::open(&path)?;
        store.set(key, b"durable".to_vec())?;
        drop(store);

        let reopened = RedbStateStore::open(&path)?;
        assert_eq!(reopened.get(&key)?, Some(b"durable".to_vec()));
        Ok(())
    }

    #[test]
    fn commit_applies_every_write_in_one_transaction() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
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
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
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
    fn enumerate_is_empty_for_a_fresh_database() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let store = RedbStateStore::open(dir.path().join("state.redb"))?;
        assert_eq!(store.enumerate()?, Vec::new());
        Ok(())
    }

    #[test]
    fn an_uncommitted_write_transaction_has_no_effect() -> StateResult<()> {
        // Demonstrates the exact `redb` guarantee `RedbStateStore::commit`'s
        // atomicity relies on: inserting into an open write transaction
        // and dropping it without calling `commit()` leaves the database
        // exactly as it was before, the same outcome as `commit()`
        // returning `Err` partway through a real `StateCommitter::commit`
        // call (ADR-0033).
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let store = RedbStateStore::open(dir.path().join("state.redb"))?;

        {
            let write_txn = store.database.begin_write().map_err(storage_error)?;
            {
                let mut table = write_txn.open_table(STATE_TABLE).map_err(storage_error)?;
                table
                    .insert([0x77_u8; 32].as_slice(), b"never-committed".as_slice())
                    .map_err(storage_error)?;
            }
            // `write_txn` dropped here without `.commit()`.
        }

        assert_eq!(store.get(&[0x77; 32])?, None);
        Ok(())
    }

    #[test]
    fn commit_block_applies_writes_and_stores_the_block_together() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
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
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
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
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
        store.commit_block(&[], &sample_finalized_block(1)?)?;
        assert_eq!(
            store.finalized_block_range(BlockHeight::new(5), 10)?,
            Vec::new()
        );
        Ok(())
    }

    #[test]
    fn latest_finalized_block_is_none_for_a_fresh_database() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let store = RedbStateStore::open(dir.path().join("state.redb"))?;
        assert_eq!(store.latest_finalized_block()?, None);
        Ok(())
    }

    #[test]
    fn latest_finalized_block_returns_the_highest_height() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let mut store = RedbStateStore::open(dir.path().join("state.redb"))?;
        for height in 1..=5 {
            store.commit_block(&[], &sample_finalized_block(height)?)?;
        }
        let latest = store
            .latest_finalized_block()?
            .ok_or_else(|| hn_state::StateError::Storage("expected a block".into()))?;
        assert_eq!(latest.envelope.header.height.get(), 5);
        Ok(())
    }

    #[test]
    fn a_block_survives_reopening_the_same_database_file() -> StateResult<()> {
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let path = dir.path().join("state.redb");
        let block = sample_finalized_block(1)?;

        let mut store = RedbStateStore::open(&path)?;
        store.commit_block(&[], &block)?;
        drop(store);

        let reopened = RedbStateStore::open(&path)?;
        assert_eq!(
            reopened.finalized_block_range(BlockHeight::new(0), 10)?,
            vec![block]
        );
        Ok(())
    }

    #[test]
    fn a_crash_mid_write_leaves_neither_the_state_write_nor_the_block_visible() -> StateResult<()> {
        // The exact crash window "Decided: Atomic Commit Of Block And
        // State" (ADR-0044) closes: inserting into both tables under
        // one open write transaction, then dropping it without ever
        // calling `.commit()` -- simulating a process death after the
        // state write but before the block write (or anywhere else
        // before the single commit point) -- must leave *neither*
        // visible afterward, never one without the other.
        let dir = tempdir().map_err(|error| hn_state::StateError::Storage(error.to_string()))?;
        let store = RedbStateStore::open(dir.path().join("state.redb"))?;
        let block = sample_finalized_block(1)?;
        let encoded_block = block.encode()?;

        {
            let write_txn = store.database.begin_write().map_err(storage_error)?;
            {
                let mut state_table = write_txn.open_table(STATE_TABLE).map_err(storage_error)?;
                state_table
                    .insert([0x88_u8; 32].as_slice(), b"never-committed".as_slice())
                    .map_err(storage_error)?;
            }
            {
                let mut blocks_table = write_txn.open_table(BLOCKS_TABLE).map_err(storage_error)?;
                blocks_table
                    .insert(1_u64, encoded_block.as_slice())
                    .map_err(storage_error)?;
            }
            // `write_txn` dropped here without `.commit()` -- simulates
            // a crash anywhere before the single commit point.
        }

        assert_eq!(store.get(&[0x88; 32])?, None);
        assert_eq!(
            store.finalized_block_range(BlockHeight::new(0), 10)?,
            Vec::new()
        );
        Ok(())
    }
}
