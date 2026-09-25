use hn_crypto::{Digest, hash_profile_0x0001};
use hn_hncs::validate_length;

use crate::error::{StateError, StateResult};

/// Maximum length, in bytes, of `BlockBody.extra_data` (ADR-0008,
/// "Decided: Extra Data Format"). Matches
/// [`crate::MAX_VOTE_METADATA_LEN`]'s own class of decision — a small,
/// bounded, rarely-used metadata field, not a bulk data channel:
/// ADR-0008's own "Extra Data" section already requires it "must not
/// become an unbounded escape hatch for consensus behavior," and this
/// bound is what actually enforces that requirement rather than just
/// stating it.
pub const MAX_EXTRA_DATA_LEN: usize = 256;

/// Computes `extra_data_hash` (ADR-0008, "Decided: Extra Data Format"):
///
/// `HASH_PROFILE_0x0001("hnchain.block.extradata.v1", extra_data)`
///
/// `extra_data` is hashed directly, not re-encoded first — it is
/// already raw bytes with no internal structure imposed at this layer
/// (ADR-0008's own reasoning for why `extra_data` carries no separate
/// version field of its own: `BlockBody.body_version` already covers
/// it, and any future content specification would version its own
/// interior, the same way `TransactionEnvelope.payload` is dispatched
/// by `tx_type` rather than carrying a redundant payload-level
/// version). Rejects an oversized `extra_data` before hashing,
/// matching every other bounded-bytes field in this codebase — an
/// empty `extra_data` (genesis's own case, since nothing has a real use
/// for this field yet) hashes without any special-cased "empty" root,
/// unlike `hn-list-merkle-v1`: this field is one opaque blob, not an
/// ordered list, so there is no tree shape to special-case.
pub fn extra_data_hash(extra_data: &[u8]) -> StateResult<Digest> {
    validate_length(extra_data.len(), MAX_EXTRA_DATA_LEN).map_err(StateError::Encoding)?;
    Ok(hash_profile_0x0001(
        "hnchain.block.extradata.v1",
        extra_data,
    )?)
}

#[cfg(test)]
mod tests {
    use super::{MAX_EXTRA_DATA_LEN, extra_data_hash};
    use crate::error::{StateError, StateResult};

    #[test]
    fn empty_extra_data_matches_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&extra_data_hash(&[])?),
            "400c15978c5b116d61a811de189f4b114d433aeb00838f230293a840bfa4ce3f"
        );
        Ok(())
    }

    #[test]
    fn real_extra_data_matches_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&extra_data_hash(b"hello")?),
            "a669453e920cec9267436d05f045a05c61d45c532389f954ad94898bddb6c949"
        );
        Ok(())
    }

    #[test]
    fn distinct_extra_data_hashes_differently() -> StateResult<()> {
        assert_ne!(extra_data_hash(b"a")?, extra_data_hash(b"b")?);
        Ok(())
    }

    #[test]
    fn rejects_extra_data_over_the_limit() {
        let oversized = vec![0_u8; MAX_EXTRA_DATA_LEN + 1];
        assert_eq!(
            extra_data_hash(&oversized),
            Err(StateError::Encoding(
                hn_hncs::HncsError::LengthLimitExceeded {
                    length: MAX_EXTRA_DATA_LEN + 1,
                    max: MAX_EXTRA_DATA_LEN,
                }
            ))
        );
    }

    #[test]
    fn accepts_extra_data_at_exactly_the_limit() -> StateResult<()> {
        let at_limit = vec![0_u8; MAX_EXTRA_DATA_LEN];
        extra_data_hash(&at_limit)?;
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
