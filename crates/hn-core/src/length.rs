use crate::{PrimitiveError, PrimitiveResult};

/// Protocol-visible byte length.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ByteLength(u32);

impl ByteLength {
    /// Maximum representable protocol byte length.
    pub const MAX: Self = Self(u32::MAX);

    /// Creates a byte length from a protocol-sized value.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Converts a host memory size into a protocol byte length.
    pub fn from_usize(value: usize) -> PrimitiveResult<Self> {
        match u32::try_from(value) {
            Ok(value) => Ok(Self(value)),
            Err(_) => Err(PrimitiveError::ByteLengthOverflow {
                value,
                max: u32::MAX,
            }),
        }
    }

    /// Returns the underlying unsigned integer value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<usize> for ByteLength {
    type Error = PrimitiveError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::from_usize(value)
    }
}

#[cfg(test)]
mod tests {
    use super::ByteLength;
    use crate::PrimitiveError;

    #[test]
    fn converts_from_usize() {
        assert_eq!(ByteLength::from_usize(32), Ok(ByteLength::new(32)));
    }

    #[test]
    fn rejects_usize_overflow() {
        let overflow = u32::MAX as usize + 1;

        assert_eq!(
            ByteLength::from_usize(overflow),
            Err(PrimitiveError::ByteLengthOverflow {
                value: overflow,
                max: u32::MAX
            })
        );
    }
}
