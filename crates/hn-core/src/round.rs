use crate::{PrimitiveError, PrimitiveResult};

/// Consensus attempt number scoped to a consensus height.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Round(u64);

impl Round {
    /// First consensus round.
    pub const FIRST: Self = Self(0);

    /// Creates a consensus round.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying unsigned integer value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next round or an overflow error.
    pub const fn checked_next(self) -> PrimitiveResult<Self> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(PrimitiveError::ArithmeticOverflow { type_name: "Round" }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Round;
    use crate::PrimitiveError;

    #[test]
    fn first_is_zero() {
        assert_eq!(Round::FIRST.get(), 0);
    }

    #[test]
    fn checked_next_increments() {
        assert_eq!(Round::new(1).checked_next(), Ok(Round::new(2)));
    }

    #[test]
    fn checked_next_rejects_overflow() {
        assert_eq!(
            Round::new(u64::MAX).checked_next(),
            Err(PrimitiveError::ArithmeticOverflow { type_name: "Round" })
        );
    }
}
