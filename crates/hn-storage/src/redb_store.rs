use std::path::Path;

use hn_crypto::Digest;
use hn_state::{StateError, StateReader, StateResult, StateWriter};
use redb::{ReadableDatabase, TableDefinition};

/// The one table this store keeps: `state_key -> canonical HNCS value
/// bytes`, exactly [`StateReader`]/[`StateWriter`]'s own scope. `&[u8]`
/// for both sides (not `[u8; 32]` for the key) — `redb`'s built-in `Key`
/// impl for `&[u8]` orders lexicographically by raw bytes, which is all
/// `StateReader`/`StateWriter` need; a fixed-width key type would gain
/// nothing here since every `state_key` this crate ever sees is already
/// exactly 32 bytes (ADR-0007).
const STATE_TABLE: TableDefinition<'_, &[u8], &[u8]> = TableDefinition::new("state");

/// A `redb`-backed durable [`StateReader`]/[`StateWriter`] implementation
/// (ADR-0019, "Decided: initial storage backend" — `redb`, a pure-Rust
/// embedded ACID key-value store, chosen over RocksDB specifically to
/// avoid requiring a C++/cmake toolchain for every contributor, and over
/// deferring the choice since ADR-0019's own phase ordering places
/// storage ahead of node/RPC/CLI work).
///
/// Every read and write goes through its own `redb` transaction —
/// matching [`StateWriter::set`]'s own single-key-at-a-time scope, not a
/// claim that a whole write set commits atomically together. ADR-0019's
/// "Atomic State Commit" rule (block header + state root + state tree
/// nodes + ... all consistent in one commit) is a [`StateCommitter`]-
/// level guarantee (ADR-0019's own boundary diagram) this crate does not
/// implement yet — the same scope limit [`crate::InMemoryStateStore`]
/// already has, not a new gap this type introduces.
///
/// [`StateCommitter`]: https://github.com/nikethebike/hnchain/blob/main/docs/adr/ADR-0019-storage-state-interfaces.md
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

/// Converts any `redb` error into [`StateError::Storage`] via its own
/// `Display` output. Backend-agnostic by construction: this crate's
/// callers see only "a storage operation failed", never a `redb`-typed
/// error, matching ADR-0019's "Backend Independence" rule.
fn storage_error(error: impl Into<redb::Error>) -> StateError {
    StateError::Storage(error.into().to_string())
}

#[cfg(test)]
mod tests {
    use hn_state::{StateReader, StateResult, StateWriter};
    use tempfile::tempdir;

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
}
