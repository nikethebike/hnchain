use hn_crypto::{Digest, hash_profile_0x0001};

use crate::error::StateResult;

/// Computes the active validator set's commitment (ADR-0010, "Validator
/// Set Commitment"):
///
/// `HASH_PROFILE_0x0001("hnchain.consensus.root.v1", HNCS(ValidatorSetCommitmentV1))`
///
/// This is **the same value** as [`crate::validator_set_commitment`] --
/// two names for one function, not two independently-computed digests.
/// `BlockHeader.consensus_root` (ADR-0008, "Consensus Root") uses this
/// name; `ConsensusVote`/`QuorumCertificate` (ADR-0012) and
/// `ConsensusEvidence` (ADR-0015) use `validator_set_commitment`. A
/// light client verifies a block's `justification` was produced by the
/// validator set the block itself claims by comparing the two fields
/// directly -- which only works because they are the same computation.
///
/// `commitment_bytes` is expected to already be the canonical HNCS
/// encoding of `ValidatorSetCommitmentV1`; this crate does not define a
/// concrete schema for it, matching how [`crate::block_hash`] and
/// [`crate::tx_id`] treat their own input.
pub fn consensus_root(commitment_bytes: &[u8]) -> StateResult<Digest> {
    Ok(hash_profile_0x0001(
        "hnchain.consensus.root.v1",
        commitment_bytes,
    )?)
}

/// Alias for [`consensus_root`] under the name used by votes, quorum
/// certificates, and evidence (ADR-0012, ADR-0015). See
/// [`consensus_root`]'s documentation for why these are the same
/// function rather than two.
pub fn validator_set_commitment(commitment_bytes: &[u8]) -> StateResult<Digest> {
    consensus_root(commitment_bytes)
}

#[cfg(test)]
mod tests {
    use super::{consensus_root, validator_set_commitment};
    use crate::error::StateResult;

    #[test]
    fn matches_independent_oracle() -> StateResult<()> {
        let commitment_bytes = b"canonical-validator-set-commitment-placeholder-v1";
        assert_eq!(
            hex(&consensus_root(commitment_bytes)?),
            "dfaccf16c1d1b40f030f38c47f1ca99eb1b460b496bf1977464c55c0026ede38"
        );
        Ok(())
    }

    #[test]
    fn consensus_root_and_validator_set_commitment_are_the_same_value() -> StateResult<()> {
        let commitment_bytes = b"some-canonical-commitment-bytes";
        assert_eq!(
            consensus_root(commitment_bytes)?,
            validator_set_commitment(commitment_bytes)?
        );
        Ok(())
    }

    #[test]
    fn distinct_commitments_hash_differently() -> StateResult<()> {
        assert_ne!(
            consensus_root(b"commitment-a")?,
            consensus_root(b"commitment-b")?
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
