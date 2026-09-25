use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use hn_crypto::{Ed25519KeyPair, KeyRole};

/// This devnet's deterministic `validator_id` for `validator_index`
/// (ADR-0037, "Decided: Devnet Validator Identity") — not a real
/// validator-onboarding mechanism, which ADR-0010 never defines.
/// Ascending `validator_index` order is already ascending `validator_id`
/// byte order (every byte equals `validator_index`), matching every
/// other consumer's ascending-`validator_id` assumption with no
/// separate sort needed.
#[must_use]
pub fn validator_id(validator_index: u8) -> [u8; 32] {
    [validator_index; 32]
}

/// This devnet validator's consensus (vote/QC signing) keypair.
#[must_use]
pub fn consensus_keypair(validator_index: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [validator_index; 32])
}

/// This devnet validator's network/handshake keypair — `ValidatorNetwork`
/// (already scoped to "validator peer-identity / network-layer
/// operations"), not the more general `NodeIdentity`, since every node
/// in this pass genuinely is a validator (ADR-0037, "Decided: Devnet
/// Validator Identity").
///
/// `KeyRole` is purely a `KeyDescriptor`-level label — `from_seed` does
/// not mix it into the actual key derivation (confirmed by reading
/// `Ed25519KeyPair::from_seed`'s own implementation: the seed bytes go
/// straight into `SigningKey::from_bytes`, `key_role` only rides along
/// on the resulting struct). Reusing `[validator_index; 32]` here, the
/// same seed [`consensus_keypair`] uses, would therefore silently
/// produce the *same* keypair under a different label, not a distinct
/// one — bitwise-inverted (`!validator_index`) keeps this seed
/// genuinely distinct for every `validator_index` this devnet cluster
/// uses (0..255 all still fit, with zero collision against
/// [`consensus_keypair`]'s own seed range for any realistic cluster
/// size).
#[must_use]
pub fn network_keypair(validator_index: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed(KeyRole::ValidatorNetwork, [!validator_index; 32])
}

/// `validator_index`'s listen address, derived from `base_port`
/// (ADR-0037, "Decided: `hn-node` Process").
#[must_use]
pub fn peer_addr(base_port: u16, validator_index: u8) -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        base_port + u16::from(validator_index),
    )
}

#[cfg(test)]
mod tests {
    use super::{consensus_keypair, network_keypair, peer_addr, validator_id};

    #[test]
    fn validator_ids_are_ascending_with_index() {
        assert!(validator_id(0) < validator_id(1));
        assert!(validator_id(1) < validator_id(2));
    }

    #[test]
    fn consensus_and_network_keys_differ_for_the_same_index() {
        let consensus = consensus_keypair(3).key_descriptor().public_key_bytes();
        let network = network_keypair(3).key_descriptor().public_key_bytes();
        assert_ne!(consensus, network);
    }

    #[test]
    fn peer_addr_offsets_by_index() {
        assert_eq!(peer_addr(30000, 0).port(), 30000);
        assert_eq!(peer_addr(30000, 3).port(), 30003);
    }
}
