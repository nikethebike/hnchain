use std::collections::BTreeMap;

use hn_crypto::Digest;
use hn_state::{StateReader, StateWriter};

/// An in-memory [`StateReader`]/[`StateWriter`] implementation.
///
/// See the crate-level documentation for what this is (a trait-boundary
/// proof) and is not (a durable backend).
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
    fn get(&self, state_key: &Digest) -> Option<Vec<u8>> {
        self.values.get(state_key).cloned()
    }
}

impl StateWriter for InMemoryStateStore {
    fn set(&mut self, state_key: Digest, value: Vec<u8>) {
        self.values.insert(state_key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::InMemoryStateStore;
    use hn_state::{StateReader, StateWriter};

    #[test]
    fn round_trips_a_stored_value() {
        let mut store = InMemoryStateStore::new();
        let key = [0x11; 32];
        store.set(key, b"hello".to_vec());
        assert_eq!(store.get(&key), Some(b"hello".to_vec()));
    }

    #[test]
    fn missing_key_reads_as_none() {
        let store = InMemoryStateStore::new();
        assert_eq!(store.get(&[0x22; 32]), None);
    }

    #[test]
    fn set_overwrites_the_previous_value() {
        let mut store = InMemoryStateStore::new();
        let key = [0x33; 32];
        store.set(key, b"first".to_vec());
        store.set(key, b"second".to_vec());
        assert_eq!(store.get(&key), Some(b"second".to_vec()));
    }
}
