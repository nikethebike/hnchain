//! Hex encoding/decoding for this binary's own text output and for
//! `address`/`tx_id` arguments a user types in.

use std::fmt;

/// Encodes `bytes` as lowercase hex, no `0x` prefix (ADR-0046's own
/// "Decided: Value Encoding Rules" convention, reused here since this
/// binary's own output should read the same way the RPC surface
/// already does).
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Why [`decode_32`] rejected its input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HexError(pub String);

impl fmt::Display for HexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for HexError {}

/// Decodes exactly 64 hex characters (either case -- unlike
/// [`crate::keyfile`]'s own seed decoding, this is a public identifier
/// a user retypes by hand, not a secret, so there is no reason to
/// reject the upper-case form) into a 32-byte value, for `address`/
/// `tx_id` command arguments.
pub fn decode_32(input: &str) -> Result<[u8; 32], HexError> {
    let bytes = input.as_bytes();
    if bytes.len() != 64 {
        return Err(HexError(format!(
            "expected 64 hex characters, got {}",
            bytes.len()
        )));
    }

    let mut decoded = [0_u8; 32];
    for (index, slot) in decoded.iter_mut().enumerate() {
        let high = nibble(bytes[index * 2])?;
        let low = nibble(bytes[index * 2 + 1])?;
        *slot = (high << 4) | low;
    }
    Ok(decoded)
}

fn nibble(byte: u8) -> Result<u8, HexError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        other => Err(HexError(format!(
            "invalid hex character: {:?}",
            other as char
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_32, encode};

    #[test]
    fn encodes_lowercase_with_no_prefix() {
        assert_eq!(encode(&[0xAB, 0x01, 0xFF]), "ab01ff");
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn decode_32_round_trips_with_encode() {
        let original = [0x7A_u8; 32];
        assert_eq!(decode_32(&encode(&original)), Ok(original));
    }

    #[test]
    fn decode_32_accepts_uppercase() {
        assert_eq!(decode_32(&"AB".repeat(32)), Ok([0xAB_u8; 32]));
    }

    #[test]
    fn decode_32_rejects_the_wrong_length() {
        assert!(decode_32(&"ab".repeat(31)).is_err());
        assert!(decode_32(&"ab".repeat(33)).is_err());
        assert!(decode_32("").is_err());
    }

    #[test]
    fn decode_32_rejects_non_hex_characters() {
        assert!(decode_32(&"zz".repeat(32)).is_err());
    }
}
