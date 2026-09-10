use hn_hncs::{HncsError, write_bytes, write_string, write_u16};
use sha2::{Digest as _, Sha512_256};

/// Digest length, in bytes, produced by every active v0.1 hash profile
/// (ADR-0005: "The initial v0.1 digest length is 32 bytes for every active
/// consensus hash domain.").
pub const DIGEST_LEN: usize = 32;

/// A digest produced by a v0.1 hash profile.
pub type Digest = [u8; DIGEST_LEN];

/// Numeric identifier of the HNChain v0.1 consensus hash profile
/// (`hn-sha512-256-v1`, ADR-0005 profile `0x0001`).
pub const HASH_PROFILE_0X0001_ID: u16 = 0x0001;

/// Maximum length, in bytes, of a domain tag `domain_name` (ADR-0005:
/// "`domain_name` maximum length is 128 bytes.").
const DOMAIN_NAME_MAX_LEN: usize = 128;

/// Maximum length, in bytes, HNCS will frame a hash input's canonical
/// payload at.
///
/// This is an implementation-level resource bound on the length field, not
/// a consensus value: it never appears in or influences a computed digest,
/// it only rejects absurdly oversized inputs before hashing.
const CANONICAL_PAYLOAD_MAX_LEN: usize = 1024 * 1024;

/// Result type for hash profile operations.
pub type HashResult<T> = Result<T, HashError>;

/// Errors produced while constructing a domain-separated hash input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HashError {
    /// The domain tag does not satisfy ADR-0005's domain tag rules: ASCII
    /// lowercase letters, digits, and `.` only, non-empty, at most 128
    /// bytes.
    InvalidDomainTag,
    /// The canonical payload could not be framed as a bounded HNCS byte
    /// sequence.
    Framing(HncsError),
}

impl From<HncsError> for HashError {
    fn from(error: HncsError) -> Self {
        Self::Framing(error)
    }
}

impl core::fmt::Display for HashError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidDomainTag => formatter.write_str("invalid domain tag"),
            Self::Framing(error) => write!(formatter, "hash input framing error: {error}"),
        }
    }
}

impl std::error::Error for HashError {}

fn validate_domain_tag(domain_tag: &str) -> HashResult<()> {
    let is_valid = !domain_tag.is_empty()
        && domain_tag.len() <= DOMAIN_NAME_MAX_LEN
        && domain_tag
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.'));

    if is_valid {
        Ok(())
    } else {
        Err(HashError::InvalidDomainTag)
    }
}

/// Encodes a `DomainTag` (ADR-0005: `u16 domain_tag_version = 1` followed by
/// the bounded UTF-8 `domain_name`).
fn write_domain_tag(out: &mut Vec<u8>, domain_tag: &str) -> HashResult<()> {
    validate_domain_tag(domain_tag)?;
    write_u16(out, 1);
    write_string(out, domain_tag, DOMAIN_NAME_MAX_LEN)?;
    Ok(())
}

/// Computes a digest under the HNChain v0.1 consensus hash profile
/// (`hn-sha512-256-v1`, ADR-0005 profile `0x0001`, SHA-512/256).
///
/// `domain_tag` must be one of the protocol's registered domain tags.
/// `canonical_payload` is the HNCS encoding of the object being committed
/// to; it is embedded length-delimited (`DomainSeparatedHashInputV1`) to
/// avoid concatenation ambiguity, per ADR-0005.
pub fn hash_profile_0x0001(domain_tag: &str, canonical_payload: &[u8]) -> HashResult<Digest> {
    let mut preimage = Vec::new();
    write_u16(&mut preimage, HASH_PROFILE_0X0001_ID);
    write_domain_tag(&mut preimage, domain_tag)?;
    write_bytes(&mut preimage, canonical_payload, CANONICAL_PAYLOAD_MAX_LEN)?;

    let digest = Sha512_256::digest(&preimage);
    let mut out: Digest = [0_u8; DIGEST_LEN];
    out.copy_from_slice(&digest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{HashResult, Sha512_256, hash_profile_0x0001};
    use sha2::Digest;

    #[test]
    fn sha512_256_matches_nist_test_vector() {
        // NIST FIPS 180-4 SHA-512/256 test vector, cross-checked against an
        // independent `hashlib.sha512_256` / `openssl dgst -sha512-256` run.
        let digest = Sha512_256::digest(b"abc");
        assert_eq!(
            hex(&digest),
            "53048e2681941ef99b2e29b76b4c7dabe4c2d0c634fc6d46e0e2f13107e7af23"
        );
    }

    #[test]
    fn hash_profile_matches_independent_oracle_for_empty_node() -> HashResult<()> {
        // hash_profile_0x0001("hnchain.state.empty.v1", HNCS(EmptyNodeV1))
        // where EmptyNodeV1 is just `u16 tree_profile = 0x0001`, computed
        // independently in Python via hashlib.sha512_256.
        let payload = 0x0001_u16.to_le_bytes();
        let digest = hash_profile_0x0001("hnchain.state.empty.v1", &payload)?;
        assert_eq!(
            hex(&digest),
            "3b94f0522d15e78871042ec41d51002c7e9dc8dd3120c22e96698cff9d4e3898"
        );
        Ok(())
    }

    #[test]
    fn domain_separates_identical_payloads() -> HashResult<()> {
        let payload = b"same-payload";
        let a = hash_profile_0x0001("hnchain.state.leaf.v1", payload)?;
        let b = hash_profile_0x0001("hnchain.state.value.v1", payload)?;
        assert_ne!(a, b);
        Ok(())
    }

    #[test]
    fn rejects_invalid_domain_tags() {
        assert!(hash_profile_0x0001("Not.Lowercase", b"").is_err());
        assert!(hash_profile_0x0001("has space", b"").is_err());
        assert!(hash_profile_0x0001("", b"").is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
