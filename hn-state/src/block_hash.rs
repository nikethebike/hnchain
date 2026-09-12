use hn_crypto::{Digest, hash_profile_0x0001};

use crate::error::StateResult;

/// Computes the block hash (ADR-0008, "Header Hash"):
///
/// `block_hash = HASH_PROFILE_0x0001("hnchain.block.header.v1", HNCS(BlockHeader))`
///
/// `header_bytes` is expected to already be the canonical HNCS encoding
/// of `BlockHeader`; this crate does not define a concrete
/// `BlockHeader` schema (several of its fields are not decided yet),
/// matching how [`crate::value_hash`] treats its own input as
/// already-canonical bytes rather than defining the schema itself.
pub fn block_hash(header_bytes: &[u8]) -> StateResult<Digest> {
    Ok(hash_profile_0x0001(
        "hnchain.block.header.v1",
        header_bytes,
    )?)
}

#[cfg(test)]
mod tests {
    use super::block_hash;
    use crate::error::StateResult;

    #[test]
    fn matches_independent_oracle() -> StateResult<()> {
        // header_bytes is a stand-in for a canonical BlockHeader
        // encoding; block_hash treats it as opaque, so any fixed byte
        // string is sufficient to cross-check the hash construction.
        let header_bytes = b"canonical-block-header-placeholder-v1";
        assert_eq!(
            hex(&block_hash(header_bytes)?),
            "d8491028137134b1004ed57ea3aaf030437407bda7026721061880cd906fed9e"
        );
        Ok(())
    }

    #[test]
    fn distinct_headers_hash_differently() -> StateResult<()> {
        assert_ne!(block_hash(b"header-a")?, block_hash(b"header-b")?);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
