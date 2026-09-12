use hn_hncs::{write_bytes, write_u8, write_u16};

use crate::hash::{Digest, HashError, HashResult, hash_profile_0x0001};

/// `address_version` for the initial address profile (ADR-0003).
pub const ADDRESS_VERSION_1: u16 = 1;

/// `address_namespace` value for the `account` namespace (ADR-0003,
/// "Namespace Separation").
pub const NAMESPACE_ACCOUNT: u8 = 0x01;

/// `derivation_scheme` registry (ADR-0003, "Derivation Scheme Separation").
///
/// ADR-0003 requires `derivation_scheme` to be explicit and distinct from
/// `algorithm_id`, but does not itself number the schemes; this assigns
/// values following the same closed-registry pattern used elsewhere
/// (`domain_id`, `address_namespace`, `chain_id`, `key_role`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DerivationScheme {
    /// `address_body` is a direct domain-separated hash commitment to the
    /// signing public key, with no additional identity commitment layer.
    DirectPublicKey = 0x01,
}

impl DerivationScheme {
    /// Returns the registry value for this scheme.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Maximum length, in bytes, of the public key bytes fed into address
/// derivation.
///
/// This is an implementation-level resource bound on the HNCS length
/// field, not a consensus value. Ed25519 (ADR-0002, `algorithm_id =
/// 0x0001`) uses 32 bytes; this stays generous ahead of any future
/// algorithm activation (for example NIST ML-DSA public keys run up to a
/// few thousand bytes) rather than being derived from an algorithm that
/// is not yet Active.
pub const PUBLIC_KEY_MAX_LEN: usize = 4096;

/// Derives the `account` namespace `address_body` (ADR-0003, "Address Body
/// Length": 32 bytes, uniform across namespaces for `address_version = 1`).
///
/// ```text
/// address_body = HASH_PROFILE_0x0001(
///   domain = "hnchain.address.account.v1",
///   payload = HNCS(AccountAddressInputV1)
/// )
///
/// AccountAddressInputV1
///   u16   address_version = 1
///   u16   network_id
///   u8    address_namespace = 0x01
///   u8    derivation_scheme
///   u16   algorithm_id
///   bytes public_key
/// ```
///
/// `public_key` is the raw signing public key bytes for `algorithm_id`
/// (ADR-0002) — not a commitment to the full `KeyDescriptor`. `key_role`
/// does not enter this derivation: the `account` namespace itself already
/// fixes the context to `account_signing`-class keys, and ADR-0002 already
/// discourages reusing one key across roles, so binding `key_role` into
/// the address would duplicate a guarantee that belongs to key management,
/// not address identity.
///
/// `algorithm_id` and a variable-length `public_key` (rather than a fixed
/// 32-byte field) are required for algorithm agility: Ed25519's 32-byte
/// key is not representative of every `algorithm_id` ADR-0002 reserves —
/// a fixed-width field would break the moment a different-length key
/// algorithm (for example a post-quantum one) activates.
pub fn account_address_body(
    network_id: u16,
    algorithm_id: u16,
    public_key: &[u8],
) -> HashResult<Digest> {
    let mut payload = Vec::new();
    write_u16(&mut payload, ADDRESS_VERSION_1);
    write_u16(&mut payload, network_id);
    write_u8(&mut payload, NAMESPACE_ACCOUNT);
    write_u8(&mut payload, DerivationScheme::DirectPublicKey.as_u8());
    write_u16(&mut payload, algorithm_id);
    write_bytes(&mut payload, public_key, PUBLIC_KEY_MAX_LEN).map_err(HashError::Framing)?;

    hash_profile_0x0001("hnchain.address.account.v1", &payload)
}

#[cfg(test)]
mod tests {
    use super::account_address_body;
    use crate::hash::HashResult;

    const PUBLIC_KEY: [u8; 32] = [
        0xd0, 0x4a, 0xb2, 0x32, 0x74, 0x2b, 0xb4, 0xab, 0x3a, 0x13, 0x68, 0xbd, 0x46, 0x15, 0xe4,
        0xe6, 0xd0, 0x22, 0x4a, 0xb7, 0x1a, 0x01, 0x6b, 0xaf, 0x85, 0x20, 0xa3, 0x32, 0xc9, 0x77,
        0x87, 0x37,
    ];

    #[test]
    fn matches_independent_oracle_for_mainnet() -> HashResult<()> {
        let address = account_address_body(0x0001, 0x0001, &PUBLIC_KEY)?;
        assert_eq!(
            hex(&address),
            "d040e6d2ad41fbbe91c3a2192642a9e4396c5c5e85ffe3f190d10aba66d7df7a"
        );
        Ok(())
    }

    #[test]
    fn matches_independent_oracle_for_testnet() -> HashResult<()> {
        let address = account_address_body(0x0002, 0x0001, &PUBLIC_KEY)?;
        assert_eq!(
            hex(&address),
            "22b1232f3fbea42e5adff0e0a955aa62f011ddd5092533bc18a64f2fd8be0fdf"
        );
        Ok(())
    }

    #[test]
    fn different_networks_derive_different_addresses() -> HashResult<()> {
        let mainnet = account_address_body(0x0001, 0x0001, &PUBLIC_KEY)?;
        let testnet = account_address_body(0x0002, 0x0001, &PUBLIC_KEY)?;
        assert_ne!(mainnet, testnet);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
