use std::path::Path;

use hn_crypto::Digest;
use hn_state::{StateCommitter, StateError, StateReader, StateResult, StateWriter, Write};
use redb::{ReadableDatabase, TableDefinition};

/// The one table this store keeps: `state_key -> canonical HNCS value
/// bytes`, exactly [`StateReader`]/[`StateWriter`]'s own scope. `&[u8]`
/// for both sides (not `[u8; 32]` for the key) — `redb`'s built-in `Key`
/// impl for `&[u8]` orders lexicographically by raw bytes, which is all
/// `StateReader`/`StateWriter` need; a fixed-width key type would gain
/// nothing here since every `state_key` this crate ever sees is already
/// exactly 32 bytes (ADR-0007).
const STATE_TABLE: TableDefinition<'_, &[u8], &[u8]> = TableDefinition::new("state");

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

/// Converts any `redb` error into [`StateError::Storage`] via its own
/// `Display` output. Backend-agnostic by construction: this crate's
/// callers see only "a storage operation failed", never a `redb`-typed
/// error, matching ADR-0019's "Backend Independence" rule.
fn storage_error(error: impl Into<redb::Error>) -> StateError {
    StateError::Storage(error.into().to_string())
}

#[cfg(test)]
mod tests {
    use hn_state::{StateCommitter, StateReader, StateResult, StateWriter, Write};
    use tempfile::tempdir;

    use super::{STATE_TABLE, storage_error};

    use super::RedbStateStore;

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
}
