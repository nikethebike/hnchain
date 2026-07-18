use core::fmt;

use crate::{PrimitiveError, PrimitiveResult};

const MAX_CHAIN_ID_LEN: usize = 32;

/// Validated HNChain network identifier.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChainId {
    value: String,
}

impl ChainId {
    /// Creates a validated chain identifier.
    pub fn new(value: &str) -> PrimitiveResult<Self> {
        validate_chain_id(value)?;
        Ok(Self {
            value: value.to_owned(),
        })
    }

    /// Returns the chain identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.value.as_str()
    }

    /// Returns the chain identifier bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.value.as_bytes()
    }

    /// Returns the chain identifier byte length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.value.len()
    }

    /// Returns true if this chain identifier is empty.
    ///
    /// A valid `ChainId` is never empty. This method exists for API symmetry
    /// with byte and string containers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}

impl AsRef<str> for ChainId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Ord for ChainId {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl PartialOrd for ChainId {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn validate_chain_id(value: &str) -> PrimitiveResult<()> {
    let bytes = value.as_bytes();

    if bytes.is_empty() {
        return Err(PrimitiveError::EmptyChainId);
    }

    if bytes.len() > MAX_CHAIN_ID_LEN {
        return Err(PrimitiveError::ChainIdTooLong {
            actual: bytes.len(),
            max: MAX_CHAIN_ID_LEN,
        });
    }

    let first = bytes.first().copied();
    if !first.is_some_and(is_ascii_lowercase_letter) {
        return Err(PrimitiveError::InvalidChainIdStart { byte: first });
    }

    let last = bytes.last().copied();
    if !last.is_some_and(is_ascii_lowercase_letter_or_digit) {
        return Err(PrimitiveError::InvalidChainIdEnd { byte: last });
    }

    let mut previous_was_hyphen = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if !is_valid_chain_id_byte(byte) {
            return Err(PrimitiveError::InvalidChainIdByte { index, byte });
        }

        if byte == b'-' && previous_was_hyphen {
            return Err(PrimitiveError::ConsecutiveHyphenInChainId { index });
        }

        previous_was_hyphen = byte == b'-';
    }

    Ok(())
}

fn is_ascii_lowercase_letter(byte: u8) -> bool {
    byte.is_ascii_lowercase()
}

fn is_ascii_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

fn is_ascii_lowercase_letter_or_digit(byte: u8) -> bool {
    is_ascii_lowercase_letter(byte) || is_ascii_digit(byte)
}

fn is_valid_chain_id_byte(byte: u8) -> bool {
    is_ascii_lowercase_letter_or_digit(byte) || byte == b'-'
}

#[cfg(test)]
mod tests {
    use super::ChainId;
    use crate::PrimitiveError;

    #[test]
    fn accepts_valid_chain_ids() {
        let mainnet = ChainId::new("hn-mainnet");
        assert_eq!(mainnet.as_ref().map(ChainId::as_str), Ok("hn-mainnet"));

        let testnet = ChainId::new("hn-testnet-1");
        assert_eq!(testnet.as_ref().map(ChainId::as_str), Ok("hn-testnet-1"));
    }

    #[test]
    fn rejects_empty_chain_id() {
        assert_eq!(ChainId::new(""), Err(PrimitiveError::EmptyChainId));
    }

    #[test]
    fn rejects_too_long_chain_id() {
        assert_eq!(
            ChainId::new("hn-mainnet-with-a-name-that-is-too-long"),
            Err(PrimitiveError::ChainIdTooLong {
                actual: 39,
                max: 32
            })
        );
    }

    #[test]
    fn rejects_invalid_chain_id_start() {
        assert_eq!(
            ChainId::new("1-mainnet"),
            Err(PrimitiveError::InvalidChainIdStart { byte: Some(b'1') })
        );
    }

    #[test]
    fn rejects_invalid_chain_id_end() {
        assert_eq!(
            ChainId::new("hn-mainnet-"),
            Err(PrimitiveError::InvalidChainIdEnd { byte: Some(b'-') })
        );
    }

    #[test]
    fn rejects_invalid_chain_id_byte() {
        assert_eq!(
            ChainId::new("hn_Mainnet"),
            Err(PrimitiveError::InvalidChainIdByte {
                index: 2,
                byte: b'_'
            })
        );
    }

    #[test]
    fn rejects_consecutive_hyphen() {
        assert_eq!(
            ChainId::new("hn--mainnet"),
            Err(PrimitiveError::ConsecutiveHyphenInChainId { index: 3 })
        );
    }

    #[test]
    fn orders_by_bytes() {
        let left = ChainId::new("hn-mainnet");
        let right = ChainId::new("hn-testnet-1");

        assert!(matches!((left, right), (Ok(left), Ok(right)) if left < right));
    }
}
