use std::collections::BTreeMap;

use hn_crypto::Digest;
use hn_state::{StateReader, StateResult, StateWriter};

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

#[cfg(test)]
mod tests {
    use super::InMemoryStateStore;
    use hn_state::{StateReader, StateResult, StateWriter};

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
}
