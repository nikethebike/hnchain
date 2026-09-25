use hn_crypto::{Digest, KeyDescriptor, hash_profile_0x0001};
use hn_hncs::{write_fixed_bytes, write_u16};

use crate::error::NetworkResult;

/// Derives a P2P peer identifier from `descriptor` (ADR-0036, "Decided:
/// Node Identity"):
///
/// `peer_id = HASH_PROFILE_0x0001("hnchain.network.peerid.v1",
/// HNCS(PeerIdInputV1))`
///
/// where `PeerIdInputV1 = { algorithm_id: u16, public_key: bytes32 }`.
///
/// Deliberately **not** bound to `network_id`/`chain_id`, unlike
/// [`hn_crypto::account_address_body`]: a peer identity is not a
/// consensus-visible object needing replay protection across networks
/// — that binding happens separately, at the handshake/envelope level
/// — and real-world P2P identity schemes are likewise network-agnostic
/// by design, letting one identity key connect to multiple networks.
pub fn peer_id(descriptor: &KeyDescriptor) -> NetworkResult<Digest> {
    let mut preimage = Vec::new();
    write_u16(&mut preimage, descriptor.algorithm_id());
    write_fixed_bytes(&mut preimage, &descriptor.public_key_bytes());

    Ok(hash_profile_0x0001("hnchain.network.peerid.v1", &preimage)?)
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::peer_id;
    use crate::error::NetworkResult;

    #[test]
    fn matches_independent_oracle() -> NetworkResult<()> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x11; 32]);
        let descriptor = keypair.key_descriptor();

        let id = peer_id(&descriptor)?;

        assert_eq!(
            hex(&id),
            "b384492353e6b88568be74efc78b9186616eeb388331c7cd3f1bb57210d4dfb6"
        );
        Ok(())
    }

    #[test]
    fn distinct_keys_derive_distinct_peer_ids() -> NetworkResult<()> {
        let a = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x11; 32]).key_descriptor();
        let b = Ed25519KeyPair::from_seed(KeyRole::NodeIdentity, [0x22; 32]).key_descriptor();
        assert_ne!(peer_id(&a)?, peer_id(&b)?);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
