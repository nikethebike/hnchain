use std::collections::BTreeMap;

use hn_crypto::Digest;

use crate::error::StateResult;
use crate::state_store::{StateReader, Write};

/// A [`StateReader`] that layers a set of not-yet-persisted writes over
/// a `base` reader (ADR-0031, "Write-Set Value Bytes And Overlay State
/// Reader") — [`crate::apply_block`]'s own fix for intra-block
/// same-sender visibility: a transaction later in a block sees every
/// write an earlier transaction in the *same* block already produced,
/// exactly as it would if those writes had already been persisted and a
/// fresh block started.
///
/// `pending` wins over `base` on a lookup hit — later writes for the
/// same `state_key` overwrite earlier ones (see [`OverlayReader::fold`]).
/// Only ever wraps one block's worth of writes; nothing here persists
/// across separate `OverlayReader` instances (cross-block visibility is
/// a distinct, still-open concern, ADR-0031's "Explicitly Not
/// Resolved").
pub struct OverlayReader<'a, R: StateReader> {
    base: &'a R,
    pending: BTreeMap<Digest, Vec<u8>>,
}

impl<'a, R: StateReader> OverlayReader<'a, R> {
    /// Creates an overlay with no writes yet — reads pass straight
    /// through to `base` until [`OverlayReader::fold`] is called.
    pub fn new(base: &'a R) -> Self {
        Self {
            base,
            pending: BTreeMap::new(),
        }
    }

    /// Records `writes` as this overlay's newest state, in order — a
    /// later entry for a `state_key` already present overwrites the
    /// earlier one, the same "last write wins within one block"
    /// semantics a real sequential state machine already has.
    pub fn fold(&mut self, writes: &[Write]) {
        for write in writes {
            self.pending.insert(write.state_key, write.value.clone());
        }
    }
}

impl<'a, R: StateReader> StateReader for OverlayReader<'a, R> {
    fn get(&self, state_key: &Digest) -> StateResult<Option<Vec<u8>>> {
        match self.pending.get(state_key) {
            Some(value) => Ok(Some(value.clone())),
            None => self.base.get(state_key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OverlayReader;
    use crate::error::StateResult;
    use crate::state_store::{StateReader, Write};

    const KEY_A: [u8; 32] = [0x11; 32];
    const KEY_B: [u8; 32] = [0x22; 32];

    struct MapReader(std::collections::BTreeMap<[u8; 32], Vec<u8>>);

    impl StateReader for MapReader {
        fn get(&self, state_key: &[u8; 32]) -> StateResult<Option<Vec<u8>>> {
            Ok(self.0.get(state_key).cloned())
        }
    }

    #[test]
    fn falls_through_to_base_when_no_pending_write_exists() -> StateResult<()> {
        let base = MapReader(std::collections::BTreeMap::from([(
            KEY_A,
            b"base-value".to_vec(),
        )]));
        let overlay = OverlayReader::new(&base);
        assert_eq!(overlay.get(&KEY_A)?, Some(b"base-value".to_vec()));
        assert_eq!(overlay.get(&KEY_B)?, None);
        Ok(())
    }

    #[test]
    fn a_pending_write_shadows_the_base_value() -> StateResult<()> {
        let base = MapReader(std::collections::BTreeMap::from([(
            KEY_A,
            b"base-value".to_vec(),
        )]));
        let mut overlay = OverlayReader::new(&base);
        overlay.fold(&[Write {
            state_key: KEY_A,
            value: b"overlay-value".to_vec(),
        }]);
        assert_eq!(overlay.get(&KEY_A)?, Some(b"overlay-value".to_vec()));
        Ok(())
    }

    #[test]
    fn a_pending_write_is_visible_for_a_key_absent_from_base() -> StateResult<()> {
        let base = MapReader(std::collections::BTreeMap::new());
        let mut overlay = OverlayReader::new(&base);
        overlay.fold(&[Write {
            state_key: KEY_B,
            value: b"new-value".to_vec(),
        }]);
        assert_eq!(overlay.get(&KEY_B)?, Some(b"new-value".to_vec()));
        Ok(())
    }

    #[test]
    fn a_later_fold_overwrites_an_earlier_write_to_the_same_key() -> StateResult<()> {
        let base = MapReader(std::collections::BTreeMap::new());
        let mut overlay = OverlayReader::new(&base);
        overlay.fold(&[Write {
            state_key: KEY_A,
            value: b"first".to_vec(),
        }]);
        overlay.fold(&[Write {
            state_key: KEY_A,
            value: b"second".to_vec(),
        }]);
        assert_eq!(overlay.get(&KEY_A)?, Some(b"second".to_vec()));
        Ok(())
    }
}
