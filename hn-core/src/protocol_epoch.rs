use crate::{PrimitiveError, PrimitiveResult};

/// Network-scoped hard-fork/activation signal (ADR-0022, "Protocol
/// Epoch"; ADR-0008, `BlockHeader.protocol_epoch`). Distinct from
/// [`crate::Epoch`], which is a consensus-protocol validator-set
/// concept, not a protocol-versioning one.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolEpoch(u64);

impl ProtocolEpoch {
    /// Genesis protocol epoch.
    pub const GENESIS: Self = Self(0);

    /// Creates a protocol epoch.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying unsigned integer value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next protocol epoch or an overflow error.
    pub const fn checked_next(self) -> PrimitiveResult<Self> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(PrimitiveError::ArithmeticOverflow {
                type_name: "ProtocolEpoch",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolEpoch;
    use crate::PrimitiveError;

    #[test]
    fn genesis_is_zero() {
        assert_eq!(ProtocolEpoch::GENESIS.get(), 0);
    }

    #[test]
    fn checked_next_increments() {
        assert_eq!(
            ProtocolEpoch::new(9).checked_next(),
            Ok(ProtocolEpoch::new(10))
        );
    }

    #[test]
    fn checked_next_rejects_overflow() {
        assert_eq!(
            ProtocolEpoch::new(u64::MAX).checked_next(),
            Err(PrimitiveError::ArithmeticOverflow {
                type_name: "ProtocolEpoch"
            })
        );
    }
}
