use crate::{PrimitiveError, PrimitiveResult};

/// HNChain protocol lineage identifier (ADR-0006, "Chain And Network
/// Binding"; ADR-0008, "Chain ID"). `u8`, a small closed registry grown
/// only through explicit governance action -- unlike [`crate::Epoch`]
/// or `network_id`'s self-assigned devnet range, there is no
/// unilateral-assignment path for a new `ChainId`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChainId(u8);

impl ChainId {
    /// The HNChain lineage, assigned at initial genesis.
    pub const HNCHAIN: Self = Self(0x01);

    /// Creates a chain identifier from a registry value. `0x00` is
    /// reserved and always invalid; every other value is structurally
    /// valid (whether it is *assigned* to a real lineage is a registry-
    /// membership question this type does not itself enforce, since the
    /// registry grows by governance action, not by a fixed protocol
    /// upgrade).
    pub const fn new(value: u8) -> PrimitiveResult<Self> {
        if value == 0 {
            return Err(PrimitiveError::ReservedChainId);
        }

        Ok(Self(value))
    }

    /// Returns the underlying registry value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::ChainId;
    use crate::PrimitiveError;

    #[test]
    fn hnchain_lineage_is_one() {
        assert_eq!(ChainId::HNCHAIN.get(), 1);
    }

    #[test]
    fn accepts_nonzero_values() {
        assert_eq!(ChainId::new(1).map(ChainId::get), Ok(1));
        assert_eq!(ChainId::new(255).map(ChainId::get), Ok(255));
    }

    #[test]
    fn rejects_reserved_zero() {
        assert_eq!(ChainId::new(0), Err(PrimitiveError::ReservedChainId));
    }
}
