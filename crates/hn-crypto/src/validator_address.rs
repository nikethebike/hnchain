use hn_hncs::{write_bytes, write_u8, write_u16};

use crate::{
    address::{ADDRESS_VERSION_1, PUBLIC_KEY_MAX_LEN},
    hash::{Digest, HashError, HashResult, hash_profile_0x0001},
};

/// `address_namespace` value for the `validator` namespace (ADR-0003,
/// "Namespace Separation").
pub const NAMESPACE_VALIDATOR: u8 = 0x03;

/// `derivation_scheme` registry for the `validator` namespace (ADR-0003,
/// "Derivation Scheme Separation").
///
/// Scoped to `validator` the same way `account`'s and `contract`'s
/// registries are scoped to their own namespaces: this independently
/// starts at `0x01` without colliding with either.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ValidatorDerivationScheme {
    /// `address_body` is a direct domain-separated hash commitment to the
    /// validator's consensus signing public key.
    DirectPublicKey = 0x01,
}

impl ValidatorDerivationScheme {
    /// Returns the registry value for this scheme.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Derives the `validator` namespace `address_body` (ADR-0003, "Address
/// Body Length": 32 bytes, uniform across namespaces for
/// `address_version = 1`).
///
/// ```text
/// address_body = HASH_PROFILE_0x0001(
///   domain = "hnchain.address.validator.v1",
///   payload = HNCS(ValidatorAddressInputV1)
/// )
///
/// ValidatorAddressInputV1
///   u16   address_version = 1
///   u16   network_id
///   u8    address_namespace = 0x03
///   u8    derivation_scheme
///   u16   algorithm_id
///   bytes public_key
/// ```
///
/// Identical in shape to [`crate::account_address_body`], except
/// `public_key` here is the validator's `validator_consensus` key
/// (ADR-0002), not an `account_signing` key, and the domain tag and
/// namespace are validator's own — this is exactly why a validator
/// address must not be treated as equivalent to an account address, per
/// ADR-0003's Validator Address section: it is an independent identity
/// from a different key under a different domain tag, not a
/// reinterpretation of the same bytes.
pub fn validator_address_body(
    network_id: u16,
    algorithm_id: u16,
    public_key: &[u8],
) -> HashResult<Digest> {
    let mut payload = Vec::new();
    write_u16(&mut payload, ADDRESS_VERSION_1);
    write_u16(&mut payload, network_id);
    write_u8(&mut payload, NAMESPACE_VALIDATOR);
    write_u8(
        &mut payload,
        ValidatorDerivationScheme::DirectPublicKey.as_u8(),
    );
    write_u16(&mut payload, algorithm_id);
    write_bytes(&mut payload, public_key, PUBLIC_KEY_MAX_LEN).map_err(HashError::Framing)?;

    hash_profile_0x0001("hnchain.address.validator.v1", &payload)
}

#[cfg(test)]
mod tests {
    use super::validator_address_body;
    use crate::hash::HashResult;

    const PUBLIC_KEY: [u8; 32] = [
        0xd0, 0x4a, 0xb2, 0x32, 0x74, 0x2b, 0xb4, 0xab, 0x3a, 0x13, 0x68, 0xbd, 0x46, 0x15, 0xe4,
        0xe6, 0xd0, 0x22, 0x4a, 0xb7, 0x1a, 0x01, 0x6b, 0xaf, 0x85, 0x20, 0xa3, 0x32, 0xc9, 0x77,
        0x87, 0x37,
    ];

    #[test]
    fn matches_independent_oracle_for_mainnet() -> HashResult<()> {
        let address = validator_address_body(0x0001, 0x0001, &PUBLIC_KEY)?;
        assert_eq!(
            hex(&address),
            "6b5403ab24419115da740fb30103c8be46efecf6141a5d381de1e9dc7de56bef"
        );
        Ok(())
    }

    #[test]
    fn matches_independent_oracle_for_testnet() -> HashResult<()> {
        let address = validator_address_body(0x0002, 0x0001, &PUBLIC_KEY)?;
        assert_eq!(
            hex(&address),
            "879dc6bdce64a4059d9062345a7a10d18ab662d08d4ab9ba2ce4064e8e66c5b0"
        );
        Ok(())
    }

    #[test]
    fn differs_from_account_address_for_same_key() -> HashResult<()> {
        let validator = validator_address_body(0x0001, 0x0001, &PUBLIC_KEY)?;
        let account = crate::account_address_body(0x0001, 0x0001, &PUBLIC_KEY)?;
        assert_ne!(validator, account);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
