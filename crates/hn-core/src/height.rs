use crate::{PrimitiveError, PrimitiveResult};

/// Position of a block in the canonical chain.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockHeight(u64);

impl BlockHeight {
    /// Genesis block height.
    pub const GENESIS: Self = Self(0);

    /// Creates a block height.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying unsigned integer value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next block height or an overflow error.
    pub const fn checked_next(self) -> PrimitiveResult<Self> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(PrimitiveError::ArithmeticOverflow {
                type_name: "BlockHeight",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BlockHeight;
    use crate::PrimitiveError;

    #[test]
    fn genesis_is_zero() {
        assert_eq!(BlockHeight::GENESIS.get(), 0);
    }

    #[test]
    fn checked_next_increments() {
        assert_eq!(
            BlockHeight::new(41).checked_next(),
            Ok(BlockHeight::new(42))
        );
    }

    #[test]
    fn checked_next_rejects_overflow() {
        assert_eq!(
            BlockHeight::new(u64::MAX).checked_next(),
            Err(PrimitiveError::ArithmeticOverflow {
                type_name: "BlockHeight"
            })
        );
    }
}
