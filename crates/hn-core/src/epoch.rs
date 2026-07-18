use crate::{PrimitiveError, PrimitiveResult};

/// Validator set and protocol scheduling period.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Epoch(u64);

impl Epoch {
    /// Genesis epoch.
    pub const GENESIS: Self = Self(0);

    /// Creates an epoch.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying unsigned integer value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next epoch or an overflow error.
    pub const fn checked_next(self) -> PrimitiveResult<Self> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(PrimitiveError::ArithmeticOverflow { type_name: "Epoch" }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Epoch;
    use crate::PrimitiveError;

    #[test]
    fn genesis_is_zero() {
        assert_eq!(Epoch::GENESIS.get(), 0);
    }

    #[test]
    fn checked_next_increments() {
        assert_eq!(Epoch::new(9).checked_next(), Ok(Epoch::new(10)));
    }

    #[test]
    fn checked_next_rejects_overflow() {
        assert_eq!(
            Epoch::new(u64::MAX).checked_next(),
            Err(PrimitiveError::ArithmeticOverflow { type_name: "Epoch" })
        );
    }
}
