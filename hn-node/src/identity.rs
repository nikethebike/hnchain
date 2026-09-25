use hn_crypto::{Digest, Ed25519KeyPair, KeyRole};

use crate::genesis::GenesisValidator;

/// Builds an [`Ed25519KeyPair`] from a raw 32-byte seed under `key_role`
/// (ADR-0038, "Decided: Node Config" — a raw seed, not a keystore file;
/// real key-management hardening is explicitly future work).
#[must_use]
pub fn keypair_from_seed(key_role: KeyRole, seed: [u8; 32]) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed(key_role, seed)
}

/// Resolves this node's own `validator_id` by matching
/// `own_consensus_key`'s public key against every genesis validator's
/// own `consensus_key` (ADR-0038, "Decided: Own Validator Identity
/// Resolution"). `None` if this node's configured consensus key does
/// not belong to any genesis validator — this pass does not support a
/// non-validating "full node" mode.
#[must_use]
pub fn resolve_own_validator_id(
    own_consensus_key: &Ed25519KeyPair,
    genesis_validators: &[GenesisValidator],
) -> Option<Digest> {
    let public_key = own_consensus_key.key_descriptor().public_key_bytes();
    genesis_validators
        .iter()
        .find(|validator| validator.consensus_key.public_key_bytes() == public_key)
        .map(|validator| validator.validator_id)
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::resolve_own_validator_id;
    use crate::genesis::GenesisValidator;

    fn validator(seed: u8) -> GenesisValidator {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [seed; 32]);
        GenesisValidator {
            validator_id: [seed; 32],
            consensus_key: keypair.key_descriptor(),
            bonded_stake: hn_state::MINIMUM_VALIDATOR_BOND,
        }
    }

    #[test]
    fn resolves_a_matching_validator() {
        let genesis_validators = vec![validator(0x01), validator(0x02)];
        let own_keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x02; 32]);
        assert_eq!(
            resolve_own_validator_id(&own_keypair, &genesis_validators),
            Some([0x02; 32])
        );
    }

    #[test]
    fn returns_none_for_an_unlisted_key() {
        let genesis_validators = vec![validator(0x01)];
        let own_keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x99; 32]);
        assert_eq!(
            resolve_own_validator_id(&own_keypair, &genesis_validators),
            None
        );
    }
}
