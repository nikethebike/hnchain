use hn_crypto::{Digest, hash_profile_0x0001};

use crate::error::StateResult;

/// Computes one validator's leaf digest for `validators_root`
/// (ADR-0010, "Validator Set Commitment"):
///
/// `HASH_PROFILE_0x0001("hnchain.validator.record.v1", HNCS(ValidatorRecordV1))`
///
/// `record_bytes` is expected to already be the canonical HNCS encoding
/// of [`crate::ValidatorRecordV1`] (`ValidatorRecordV1::encode()`'s
/// output) — this function still treats it as opaque bytes rather than
/// taking a `&ValidatorRecordV1` directly, matching how
/// [`crate::block_hash`] and [`crate::tx_id`] treat their own input as
/// already-canonical bytes rather than defining/consuming the schema
/// itself.
///
/// Callers combine these digests, sorted by ascending `validator_id`,
/// into `validators_root` via [`crate::list_merkle_root`].
pub fn validator_digest(record_bytes: &[u8]) -> StateResult<Digest> {
    Ok(hash_profile_0x0001(
        "hnchain.validator.record.v1",
        record_bytes,
    )?)
}

#[cfg(test)]
mod tests {
    use super::validator_digest;
    use crate::error::StateResult;

    #[test]
    fn matches_independent_oracle() -> StateResult<()> {
        let record_bytes = b"canonical-validator-record-placeholder-v1";
        assert_eq!(
            hex(&validator_digest(record_bytes)?),
            "554ad0085fa4dc9d8a5fca1f30095a4a7f2d20baedcfe8adbf64329237787388"
        );
        Ok(())
    }

    #[test]
    fn distinct_records_digest_differently() -> StateResult<()> {
        assert_ne!(
            validator_digest(b"record-a")?,
            validator_digest(b"record-b")?
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
