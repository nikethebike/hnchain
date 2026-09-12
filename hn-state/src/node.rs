use hn_crypto::{Digest, hash_profile_0x0001};
use hn_hncs::write_u16;

use crate::error::StateResult;

/// Tree profile identifier for `hn-smt-256-v1` (ADR-0007).
pub const TREE_PROFILE_ID: u16 = 0x0001;

/// Tree depth, in bits, for `hn-smt-256-v1` (ADR-0007: "Tree Depth: 256 bits").
pub const TREE_DEPTH: usize = 256;

/// Computes a value hash (ADR-0007, "The value hash is"):
///
/// `value_hash = HASH_PROFILE_0x0001("hnchain.state.value.v1", HNCS(StateValueV1))`
///
/// `value` is expected to already be the canonical HNCS encoding of the
/// committed value; this crate does not define a concrete `StateValueV1`
/// schema, since value schemas are owned by each state domain.
pub fn value_hash(value: &[u8]) -> StateResult<Digest> {
    Ok(hash_profile_0x0001("hnchain.state.value.v1", value)?)
}

/// Computes a leaf preimage hash (ADR-0007, "Leaf Preimage"):
///
/// `LeafHash = HASH_PROFILE_0x0001("hnchain.state.leaf.v1", HNCS(LeafNodeV1))`
pub fn leaf_hash(state_key: &Digest, value_hash: &Digest) -> StateResult<Digest> {
    let mut payload = Vec::with_capacity(2 + state_key.len() + value_hash.len());
    write_u16(&mut payload, TREE_PROFILE_ID);
    payload.extend_from_slice(state_key);
    payload.extend_from_slice(value_hash);

    Ok(hash_profile_0x0001("hnchain.state.leaf.v1", &payload)?)
}

/// Computes an internal node hash (ADR-0007, "Internal Node Child Order"):
/// the preimage order is always `left_child_hash || right_child_hash`.
pub fn internal_hash(left: &Digest, right: &Digest) -> StateResult<Digest> {
    let mut payload = Vec::with_capacity(2 + left.len() + right.len());
    write_u16(&mut payload, TREE_PROFILE_ID);
    payload.extend_from_slice(left);
    payload.extend_from_slice(right);

    Ok(hash_profile_0x0001("hnchain.state.internal.v1", &payload)?)
}

/// Precomputed empty-subtree hashes for every depth `0..=256`
/// (ADR-0007, "Empty Root Construction").
pub struct EmptyHashTable {
    hashes: [Digest; TREE_DEPTH + 1],
}

impl EmptyHashTable {
    /// Builds the table bottom-up: `empty_hash[0]` is the empty node hash,
    /// and each `empty_hash[d]` is the internal hash of two `empty_hash[d-1]`
    /// children.
    pub fn build() -> StateResult<Self> {
        let mut hashes = [[0_u8; hn_crypto::DIGEST_LEN]; TREE_DEPTH + 1];

        let mut empty_node_payload = Vec::new();
        write_u16(&mut empty_node_payload, TREE_PROFILE_ID);
        hashes[0] = hash_profile_0x0001("hnchain.state.empty.v1", &empty_node_payload)?;

        for depth in 1..=TREE_DEPTH {
            hashes[depth] = internal_hash(&hashes[depth - 1], &hashes[depth - 1])?;
        }

        Ok(Self { hashes })
    }

    /// Returns the empty-subtree hash at `depth` (`0..=256`).
    pub fn get(&self, depth: usize) -> Digest {
        self.hashes[depth]
    }

    /// Returns `empty_hash[256]`, the state root of a tree with no
    /// committed state keys.
    pub fn empty_root(&self) -> Digest {
        self.hashes[TREE_DEPTH]
    }
}

#[cfg(test)]
mod tests {
    use super::EmptyHashTable;
    use crate::error::StateResult;

    #[test]
    fn matches_independent_oracle_for_empty_root() -> StateResult<()> {
        let table = EmptyHashTable::build()?;
        assert_eq!(
            hex(&table.get(0)),
            "3b94f0522d15e78871042ec41d51002c7e9dc8dd3120c22e96698cff9d4e3898"
        );
        assert_eq!(
            hex(&table.empty_root()),
            "03c138bed1796de30f8897ff54e1289597fa9bd5d924dc1695967502a85b9787"
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
