use crate::{PrimitiveError, PrimitiveResult};

/// Account transaction nonce.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AccountNonce(u64);

impl AccountNonce {
    /// Initial account nonce.
    pub const INITIAL: Self = Self(0);

    /// Creates an account nonce.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying unsigned integer value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next nonce or an overflow error.
    pub const fn checked_next(self) -> PrimitiveResult<Self> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(PrimitiveError::ArithmeticOverflow {
                type_name: "AccountNonce",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AccountNonce;
    use crate::PrimitiveError;

    #[test]
    fn initial_is_zero() {
        assert_eq!(AccountNonce::INITIAL.get(), 0);
    }

    #[test]
    fn checked_next_increments() {
        assert_eq!(
            AccountNonce::new(4).checked_next(),
            Ok(AccountNonce::new(5))
        );
    }

    #[test]
    fn checked_next_rejects_overflow() {
        assert_eq!(
            AccountNonce::new(u64::MAX).checked_next(),
            Err(PrimitiveError::ArithmeticOverflow {
                type_name: "AccountNonce"
            })
        );
    }
}
