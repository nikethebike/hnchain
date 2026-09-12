use hn_crypto::Digest;

use crate::{
    error::{StateError, StateResult},
    node::{EmptyHashTable, TREE_DEPTH, internal_hash},
};

/// One `(state_key, leaf_hash)` entry in a write set passed to
/// [`compute_state_root`].
pub type Leaf = (Digest, Digest);

/// Computes the `hn-smt-256-v1` state root for a finished write set
/// (ADR-0007, "Updates": "The state tree layer accepts a deterministic
/// final write set; it does not resolve transaction conflicts.").
///
/// `entries` order does not affect the result: the root is a pure function
/// of the `(state_key, leaf_hash)` set, not of insertion order.
///
/// Returns [`StateError::DuplicateStateKey`] if two entries share the same
/// `state_key`.
pub fn compute_state_root(entries: &[Leaf], empty_table: &EmptyHashTable) -> StateResult<Digest> {
    let mut keys: Vec<Digest> = entries.iter().map(|(key, _)| *key).collect();
    keys.sort_unstable();
    if keys.windows(2).any(|window| window[0] == window[1]) {
        return Err(StateError::DuplicateStateKey);
    }

    subtree_root(TREE_DEPTH, entries, empty_table)
}

fn subtree_root(
    depth: usize,
    entries: &[Leaf],
    empty_table: &EmptyHashTable,
) -> StateResult<Digest> {
    let Some(first) = entries.first() else {
        return Ok(empty_table.get(depth));
    };

    if depth == 0 {
        return Ok(first.1);
    }

    let bit_index = TREE_DEPTH - depth;
    let mut left = Vec::new();
    let mut right = Vec::new();
    for entry in entries {
        if bit_at(&entry.0, bit_index) == 0 {
            left.push(*entry);
        } else {
            right.push(*entry);
        }
    }

    let left_root = subtree_root(depth - 1, &left, empty_table)?;
    let right_root = subtree_root(depth - 1, &right, empty_table)?;
    internal_hash(&left_root, &right_root)
}

/// Reads bit `index` (`0` = most significant bit of byte `0`) of a 256-bit
/// state key (ADR-0007, "Internal Node Child Order").
fn bit_at(key: &Digest, index: usize) -> u8 {
    let byte_index = index / 8;
    let bit_in_byte = 7 - (index % 8);
    (key[byte_index] >> bit_in_byte) & 1
}

#[cfg(test)]
mod tests {
    use super::compute_state_root;
    use crate::{
        error::{StateError, StateResult},
        key::state_key_core,
        node::{EmptyHashTable, leaf_hash, value_hash},
    };

    #[test]
    fn empty_write_set_yields_empty_root() -> StateResult<()> {
        let table = EmptyHashTable::build()?;
        let root = compute_state_root(&[], &table)?;
        assert_eq!(root, table.empty_root());
        Ok(())
    }

    #[test]
    fn root_is_independent_of_insertion_order() -> StateResult<()> {
        let table = EmptyHashTable::build()?;
        let object_id = [0x11_u8; 32];

        let mut leaves = Vec::new();
        for section_id in [0x00_u8, 0x02_u8, 0x03_u8] {
            let key = state_key_core(0x01, section_id, &object_id, &[])?;
            let value = value_hash(format!("section-{section_id}").as_bytes())?;
            leaves.push((key, leaf_hash(&key, &value)?));
        }

        let forward = compute_state_root(&leaves, &table)?;
        leaves.reverse();
        let reversed = compute_state_root(&leaves, &table)?;

        assert_eq!(forward, reversed);
        Ok(())
    }

    #[test]
    fn rejects_duplicate_state_keys() -> StateResult<()> {
        let table = EmptyHashTable::build()?;
        let object_id = [0x11_u8; 32];
        let key = state_key_core(0x01, 0x00, &object_id, &[])?;
        let value_one = leaf_hash(&key, &value_hash(b"one")?)?;
        let value_two = leaf_hash(&key, &value_hash(b"two")?)?;

        let result = compute_state_root(&[(key, value_one), (key, value_two)], &table);
        assert_eq!(result, Err(StateError::DuplicateStateKey));
        Ok(())
    }
}
