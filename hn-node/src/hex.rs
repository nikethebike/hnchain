//! Minimal hex decoding for config and genesis file fields (ADR-0038)
//! — no existing hex utility exists anywhere else in this workspace
//! (every other crate's own `hex()` helper only *encodes*, for test
//! assertion messages); this one is real, production-facing decode
//! logic, the direction those never needed.

/// Decodes a hex string (even length, `0-9a-fA-F` only, no `0x` prefix)
/// into bytes. `None` on any malformed input.
pub fn decode(input: &str) -> Option<Vec<u8>> {
    if !input.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(input.len() / 2);
    let chars: Vec<u8> = input.bytes().collect();
    for pair in chars.chunks(2) {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn round_trips() {
        let bytes = [0x00_u8, 0x01, 0xAB, 0xFF];
        assert_eq!(decode(&encode(&bytes)), Some(bytes.to_vec()));
    }

    #[test]
    fn decode_accepts_mixed_case() {
        assert_eq!(decode("aAbBcCdD"), decode("AABBCCDD"));
    }

    #[test]
    fn decode_rejects_odd_length() {
        assert_eq!(decode("abc"), None);
    }

    #[test]
    fn decode_rejects_non_hex_characters() {
        assert_eq!(decode("zz"), None);
    }

    #[test]
    fn decode_empty_is_empty() {
        assert_eq!(decode(""), Some(Vec::new()));
    }
}
