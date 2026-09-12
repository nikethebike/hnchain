use crate::hash::Digest;

/// `address_namespace` value for the `protocol` namespace (ADR-0003,
/// "Namespace Separation").
pub const NAMESPACE_PROTOCOL: u8 = 0x04;

/// The `address_version = 1` genesis module registry (ADR-0003, Protocol
/// Address, "Decided: protocol address is genesis-assigned, not
/// derived").
///
/// Unlike `account`/`contract`/`validator`, there is no `HASH_PROFILE_0x0001`
/// derivation for `protocol` addresses: there is no key or deployment
/// input to derive from, only a small, deliberately reserved module list
/// assigned directly by the genesis specification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ProtocolModule {
    /// Protocol treasury (`system` state domain, `0x0009` — a singleton).
    Treasury = 0x01,
    /// Governance system contracts (`governance` state domain, `0x0007`
    /// — a singleton).
    Governance = 0x02,
    /// Staking records (`validators` state domain, `0x0006` — per-
    /// validator/delegator records).
    Staking = 0x03,
    /// Slashing penalty history (`validators` state domain, `0x0006` —
    /// per-validator records).
    Slashing = 0x04,
    /// Bridge registry: which external chains and assets are supported,
    /// custody rules (`bridge` state domain, `0x000A` — a singleton).
    BridgeRegistry = 0x05,
}

impl ProtocolModule {
    /// Returns this module's `module_id`.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Returns this module's fixed, genesis-assigned `address_body`:
    /// 31 zero bytes followed by `module_id`.
    pub fn address_body(self) -> Digest {
        let mut body = [0_u8; 32];
        body[31] = self.as_u8();
        body
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolModule;

    #[test]
    fn address_bodies_are_pairwise_distinct() {
        let modules = [
            ProtocolModule::Treasury,
            ProtocolModule::Governance,
            ProtocolModule::Staking,
            ProtocolModule::Slashing,
            ProtocolModule::BridgeRegistry,
        ];

        let bodies: Vec<_> = modules.iter().map(|module| module.address_body()).collect();
        for (i, a) in bodies.iter().enumerate() {
            for b in &bodies[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn treasury_is_the_expected_fixed_constant() {
        let mut expected = [0_u8; 32];
        expected[31] = 0x01;
        assert_eq!(ProtocolModule::Treasury.address_body(), expected);
    }

    #[test]
    fn address_body_is_mostly_zero() {
        let body = ProtocolModule::BridgeRegistry.address_body();
        assert!(body[..31].iter().all(|&byte| byte == 0));
        assert_eq!(body[31], 0x05);
    }
}
