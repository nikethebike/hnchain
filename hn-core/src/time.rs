/// Protocol-facing UTC Unix timestamp in milliseconds.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixTimeMillis(u64);

impl UnixTimeMillis {
    /// Unix epoch timestamp.
    pub const UNIX_EPOCH: Self = Self(0);

    /// Creates a timestamp from milliseconds since `1970-01-01T00:00:00Z`.
    #[must_use]
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    /// Returns milliseconds since `1970-01-01T00:00:00Z`.
    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::UnixTimeMillis;

    #[test]
    fn unix_epoch_is_zero() {
        assert_eq!(UnixTimeMillis::UNIX_EPOCH.as_millis(), 0);
    }

    #[test]
    fn exposes_millis() {
        assert_eq!(
            UnixTimeMillis::from_millis(1_700_000_000_000).as_millis(),
            1_700_000_000_000
        );
    }
}
