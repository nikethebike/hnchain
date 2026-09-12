use hn_hncs::{write_bytes, write_u8, write_u16};

use crate::{
    address::ADDRESS_VERSION_1,
    hash::{Digest, HashError, HashResult, hash_profile_0x0001},
};

/// `address_namespace` value for the `contract` namespace (ADR-0003,
/// "Namespace Separation").
pub const NAMESPACE_CONTRACT: u8 = 0x02;

/// `derivation_scheme` registry for the `contract` namespace (ADR-0003,
/// "Derivation Scheme Separation").
///
/// Scoped to `contract` the same way ADR-0007 scopes `section_id` to a
/// `domain_id`: a `derivation_scheme` value is only meaningful together
/// with the `address_namespace` it was derived under, so `contract`'s
/// registry independently starts at `0x01`, the same value `account`'s
/// [`crate::DerivationScheme`] registry starts at, without colliding —
/// the two are different registries, not one shared one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ContractDerivationScheme {
    /// `address_body` binds the deployer's account address, the deployed
    /// code's commitment, and a caller-supplied unique deployment input.
    DeployerCodeCommitment = 0x01,
}

impl ContractDerivationScheme {
    /// Returns the registry value for this scheme.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Maximum length, in bytes, of the `deployment_input` fed into contract
/// address derivation.
///
/// This is an implementation-level resource bound on the HNCS length
/// field, not a consensus value. ADR-0003's Contract Address section
/// requires binding to a "deployment nonce or unique deployment input"
/// without fixing its shape (the account nonce model itself is still
/// undecided — account-state.md SS4.4), so this function treats it as an
/// opaque bounded byte string rather than assuming a specific nonce width.
pub const DEPLOYMENT_INPUT_MAX_LEN: usize = 128;

/// Derives the `contract` namespace `address_body` (ADR-0003, "Address
/// Body Length": 32 bytes, uniform across namespaces for
/// `address_version = 1`).
///
/// ```text
/// address_body = HASH_PROFILE_0x0001(
///   domain = "hnchain.address.contract.v1",
///   payload = HNCS(ContractAddressInputV1)
/// )
///
/// ContractAddressInputV1
///   u16     address_version = 1
///   u16     network_id
///   u8      address_namespace = 0x02
///   u8      derivation_scheme
///   bytes32 deployer_address_body
///   bytes32 code_commitment
///   bytes   deployment_input
/// ```
///
/// `deployer_address_body` is the deployer account's own already-derived
/// 32-byte `address_body` ([`crate::account_address_body`]), not the
/// deployer's raw public key: by the time a contract is deployed, the
/// deployer is identified by its account address, the same way ADR-0003's
/// Contract Address section names "creator or deployer account" as the
/// binding input, not a key.
///
/// `code_commitment` is a 32-byte domain-separated hash commitment to the
/// canonical deployed code bytes; this function takes it as given and
/// does not itself define how it is computed.
///
/// `network_id` binds the same way it does for
/// [`crate::account_address_body`]: ADR-0003 lists "network identifier"
/// as one of Contract Address's five required binding inputs, not an
/// account-specific detail that stops applying here.
pub fn contract_address_body(
    network_id: u16,
    deployer_address_body: &Digest,
    code_commitment: &Digest,
    deployment_input: &[u8],
) -> HashResult<Digest> {
    let mut payload = Vec::new();
    write_u16(&mut payload, ADDRESS_VERSION_1);
    write_u16(&mut payload, network_id);
    write_u8(&mut payload, NAMESPACE_CONTRACT);
    write_u8(
        &mut payload,
        ContractDerivationScheme::DeployerCodeCommitment.as_u8(),
    );
    payload.extend_from_slice(deployer_address_body);
    payload.extend_from_slice(code_commitment);
    write_bytes(&mut payload, deployment_input, DEPLOYMENT_INPUT_MAX_LEN)
        .map_err(HashError::Framing)?;

    hash_profile_0x0001("hnchain.address.contract.v1", &payload)
}

#[cfg(test)]
mod tests {
    use super::contract_address_body;
    use crate::hash::HashResult;

    // The mainnet account address_body derived earlier for public key
    // [0xd0,0x4a,0xb2,...] (address::tests::matches_independent_oracle_for_mainnet),
    // used here as an already-derived deployer *address*, not a raw key -
    // ADR-0003's Contract Address binds to "creator or deployer account",
    // i.e. an address, not a public key.
    const DEPLOYER: [u8; 32] = [
        0xd0, 0x40, 0xe6, 0xd2, 0xad, 0x41, 0xfb, 0xbe, 0x91, 0xc3, 0xa2, 0x19, 0x26, 0x42, 0xa9,
        0xe4, 0x39, 0x6c, 0x5c, 0x5e, 0x85, 0xff, 0xe3, 0xf1, 0x90, 0xd1, 0x0a, 0xba, 0x66, 0xd7,
        0xdf, 0x7a,
    ];
    const CODE_COMMITMENT: [u8; 32] = [0xAB; 32];

    fn nonce_bytes(value: u64) -> [u8; 8] {
        value.to_le_bytes()
    }

    #[test]
    fn matches_independent_oracle_for_mainnet() -> HashResult<()> {
        let address = contract_address_body(0x0001, &DEPLOYER, &CODE_COMMITMENT, &nonce_bytes(0))?;
        assert_eq!(
            hex(&address),
            "9c1c2a1c75dab943aa4996216bcee87bdf321b167e459c1b39839943101161f6"
        );
        Ok(())
    }

    #[test]
    fn matches_independent_oracle_for_testnet() -> HashResult<()> {
        let address = contract_address_body(0x0002, &DEPLOYER, &CODE_COMMITMENT, &nonce_bytes(0))?;
        assert_eq!(
            hex(&address),
            "98215239e18b344bf7e4ea4856bdba5debbd05643cff267de1bbfa594cc9d4b1"
        );
        Ok(())
    }

    #[test]
    fn matches_independent_oracle_for_different_nonce() -> HashResult<()> {
        let address = contract_address_body(0x0001, &DEPLOYER, &CODE_COMMITMENT, &nonce_bytes(1))?;
        assert_eq!(
            hex(&address),
            "542f811d21ad07ad4c76924761ceed8a25ce5dc0451166cbb0eebfb239559f04"
        );
        Ok(())
    }

    #[test]
    fn different_networks_derive_different_contract_addresses() -> HashResult<()> {
        let mainnet = contract_address_body(0x0001, &DEPLOYER, &CODE_COMMITMENT, &nonce_bytes(0))?;
        let testnet = contract_address_body(0x0002, &DEPLOYER, &CODE_COMMITMENT, &nonce_bytes(0))?;
        assert_ne!(mainnet, testnet);
        Ok(())
    }

    #[test]
    fn different_deployment_inputs_derive_different_addresses() -> HashResult<()> {
        let nonce_0 = contract_address_body(0x0001, &DEPLOYER, &CODE_COMMITMENT, &nonce_bytes(0))?;
        let nonce_1 = contract_address_body(0x0001, &DEPLOYER, &CODE_COMMITMENT, &nonce_bytes(1))?;
        assert_ne!(nonce_0, nonce_1);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
