use hn_crypto::{Digest, hash_profile_0x0001};

use crate::error::StateResult;

/// Computes one evidence object's leaf digest for `evidence_root`
/// (ADR-0015, "Versioned Evidence"):
///
/// `evidence_hash = HASH_PROFILE_0x0001("hnchain.evidence.v1", HNCS(ConsensusEvidence))`
///
/// `evidence_bytes` is expected to already be the canonical HNCS
/// encoding of `ConsensusEvidence`; this crate does not define a
/// concrete `ConsensusEvidence` schema, matching how
/// [`crate::validator_digest`] treats its own input.
///
/// Callers combine these digests, sorted by ascending digest value (not
/// inclusion order -- evidence order carries no consensus meaning,
/// unlike `transactions_root`), into `evidence_root` via
/// [`crate::list_merkle_root`].
pub fn evidence_digest(evidence_bytes: &[u8]) -> StateResult<Digest> {
    Ok(hash_profile_0x0001("hnchain.evidence.v1", evidence_bytes)?)
}

#[cfg(test)]
mod tests {
    use super::evidence_digest;
    use crate::error::StateResult;

    #[test]
    fn matches_independent_oracle() -> StateResult<()> {
        let evidence_bytes = b"canonical-evidence-placeholder-v1";
        assert_eq!(
            hex(&evidence_digest(evidence_bytes)?),
            "13086fa4ff667f957fb3de13785a4824daa8097c24074b6762b154dda499ac49"
        );
        Ok(())
    }

    #[test]
    fn distinct_evidence_digests_differently() -> StateResult<()> {
        assert_ne!(
            evidence_digest(b"evidence-a")?,
            evidence_digest(b"evidence-b")?
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
