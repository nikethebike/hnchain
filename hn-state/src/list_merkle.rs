use hn_crypto::{Digest, hash_profile_0x0001};
use hn_hncs::write_u16;

use crate::error::StateResult;

/// Tree profile identifier for `hn-list-merkle-v1` (ADR-0008, "Ordered
/// List Commitment"). Independent registry from ADR-0007's
/// `TREE_PROFILE_ID` — the two profiles are never compared against
/// each other.
pub const LIST_TREE_PROFILE_ID: u16 = 0x0001;

/// Computes the root of an ordered list of leaf digests (`MTH`,
/// ADR-0008, "Ordered List Commitment"). Leaves are expected to already
/// be canonical, domain-separated digests (for example `tx_id` for
/// `transactions_root`, or a receipt digest for `receipts_root`) — this
/// function does not hash them again.
///
/// Follows RFC 6962 (Certificate Transparency) exactly: for `n > 1`,
/// splits at the largest power of two less than `n` and combines the
/// two subtree roots. This never duplicates an unpaired leaf to force a
/// balanced pairing — that construction is what CVE-2012-2459 exploited
/// in early Bitcoin, where a block containing a duplicated transaction
/// could produce the same Merkle root as a differently-shaped block.
pub fn list_merkle_root(leaves: &[Digest]) -> StateResult<Digest> {
    mth(leaves)
}

fn mth(leaves: &[Digest]) -> StateResult<Digest> {
    match leaves.len() {
        0 => list_empty_root(),
        1 => Ok(leaves[0]),
        n => {
            let k = largest_power_of_two_less_than(n);
            let left = mth(&leaves[..k])?;
            let right = mth(&leaves[k..])?;
            list_node_hash(&left, &right)
        }
    }
}

/// The largest power of two strictly less than `n` (`n` must be `>= 2`).
fn largest_power_of_two_less_than(n: usize) -> usize {
    debug_assert!(n >= 2);
    let mut k = 1_usize;
    while k * 2 < n {
        k *= 2;
    }
    k
}

/// Computes an internal node hash for `hn-list-merkle-v1` (ADR-0008,
/// "Ordered List Commitment").
pub fn list_node_hash(left: &Digest, right: &Digest) -> StateResult<Digest> {
    let mut payload = Vec::with_capacity(2 + left.len() + right.len());
    write_u16(&mut payload, LIST_TREE_PROFILE_ID);
    payload.extend_from_slice(left);
    payload.extend_from_slice(right);

    Ok(hash_profile_0x0001("hnchain.list.node.v1", &payload)?)
}

/// Computes the empty-list root for `hn-list-merkle-v1` (ADR-0008,
/// "Ordered List Commitment").
pub fn list_empty_root() -> StateResult<Digest> {
    let mut payload = Vec::with_capacity(2);
    write_u16(&mut payload, LIST_TREE_PROFILE_ID);

    Ok(hash_profile_0x0001("hnchain.list.empty.v1", &payload)?)
}

#[cfg(test)]
mod tests {
    use super::list_merkle_root;
    use crate::error::StateResult;

    /// Leaf `i` is 32 bytes of value `i` repeated — synthetic but
    /// deterministic, matching the independent Python oracle exactly.
    fn leaf(i: u8) -> [u8; 32] {
        [i; 32]
    }

    #[test]
    fn n0_empty_list_matches_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&list_merkle_root(&[])?),
            "238b80f024ac5b67fdf0254357d61e836b61fb5ee03ced8e2d4113fb0aa005c1"
        );
        Ok(())
    }

    #[test]
    fn n1_single_leaf_is_the_leaf_itself() -> StateResult<()> {
        let leaves = [leaf(0)];
        assert_eq!(list_merkle_root(&leaves)?, leaf(0));
        Ok(())
    }

    #[test]
    fn n2_matches_independent_oracle() -> StateResult<()> {
        let leaves = [leaf(0), leaf(1)];
        assert_eq!(
            hex(&list_merkle_root(&leaves)?),
            "e4f510465411a04973b0cf82f546fd6c6aa2e2de6fc4da5237a974bf5a9a5f13"
        );
        Ok(())
    }

    #[test]
    fn n3_uneven_split_matches_independent_oracle() -> StateResult<()> {
        let leaves = [leaf(0), leaf(1), leaf(2)];
        assert_eq!(
            hex(&list_merkle_root(&leaves)?),
            "d6f148823fbdcec7bb9a413e64c2a9d2c290ddb4f180d9f44cba02d12f50537d"
        );
        Ok(())
    }

    #[test]
    fn n4_perfect_power_of_two_matches_independent_oracle() -> StateResult<()> {
        let leaves = [leaf(0), leaf(1), leaf(2), leaf(3)];
        assert_eq!(
            hex(&list_merkle_root(&leaves)?),
            "81fea152bb2de589f624aa4035fc7bf388d7b422045e3d9892310efe7e4f2ae0"
        );
        Ok(())
    }

    #[test]
    fn n5_matches_independent_oracle() -> StateResult<()> {
        let leaves = [leaf(0), leaf(1), leaf(2), leaf(3), leaf(4)];
        assert_eq!(
            hex(&list_merkle_root(&leaves)?),
            "52948ce05283be5b535fbc394cad93499a8cbd9734bd1928444e821c6ddb3e0e"
        );
        Ok(())
    }

    #[test]
    fn n6_matches_independent_oracle() -> StateResult<()> {
        let leaves: Vec<_> = (0..6).map(leaf).collect();
        assert_eq!(
            hex(&list_merkle_root(&leaves)?),
            "3e7a7ee609ce54af82eb91df2bde86cf7b77021121ea4203c7b24173d08065a2"
        );
        Ok(())
    }

    #[test]
    fn n7_matches_independent_oracle() -> StateResult<()> {
        let leaves: Vec<_> = (0..7).map(leaf).collect();
        assert_eq!(
            hex(&list_merkle_root(&leaves)?),
            "a41466c85d5b5bc6be7d6ce7a681ecf994d1d76e5452b9cdb7a28f630fa71aeb"
        );
        Ok(())
    }

    #[test]
    fn n8_perfect_power_of_two_matches_independent_oracle() -> StateResult<()> {
        let leaves: Vec<_> = (0..8).map(leaf).collect();
        assert_eq!(
            hex(&list_merkle_root(&leaves)?),
            "9086514f82412f37116c3c907eace841ca3e074c4de098aa14c99252967313bf"
        );
        Ok(())
    }

    #[test]
    fn root_depends_on_order() -> StateResult<()> {
        let forward = [leaf(0), leaf(1), leaf(2)];
        let reversed = [leaf(2), leaf(1), leaf(0)];
        assert_ne!(list_merkle_root(&forward)?, list_merkle_root(&reversed)?);
        Ok(())
    }

    #[test]
    fn root_depends_on_count_not_just_content() -> StateResult<()> {
        // A 4-leaf list where the last leaf repeats the third must not
        // collide with the 3-leaf list of just the first three --
        // guards against exactly the CVE-2012-2459 duplication class.
        let three = [leaf(0), leaf(1), leaf(2)];
        let four_with_duplicate = [leaf(0), leaf(1), leaf(2), leaf(2)];
        assert_ne!(
            list_merkle_root(&three)?,
            list_merkle_root(&four_with_duplicate)?
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
