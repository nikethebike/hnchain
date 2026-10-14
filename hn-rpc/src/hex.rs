/// Encodes `bytes` as lowercase hex, no `0x` prefix (ADR-0046,
/// "Decided: Value Encoding Rules" — "Byte strings ...: lowercase
/// hex, no `0x` prefix").
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0F));
    }
    out
}

/// Decodes `text` as hex into exactly 32 bytes — the shape every
/// address, transaction id, or block hash in this surface takes
/// (ADR-0003/ADR-0006/ADR-0008). `None` for the wrong length or any
/// non-hex character; the caller maps that to its own error code
/// (ADR-0046's own `400 invalid_address` for an address specifically).
#[must_use]
pub fn decode_32(text: &str) -> Option<[u8; 32]> {
    let decoded = decode(text)?;
    decoded.try_into().ok()
}

/// Decodes `text` as hex into bytes of any length — used for the
/// submitted transaction's own canonical bytes, whose length this
/// crate does not itself bound (`hn_state::MAX_TRANSACTION_SIZE` is a
/// `hn_state` constant this crate does not depend on; the decoded
/// length is instead bounded indirectly by this server's own request
/// body size limit).
#[must_use]
pub fn decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_to_nibble(pair[0])?;
        let low = hex_to_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Some(out)
}

const fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'a' + nibble - 10) as char,
    }
}

const fn hex_to_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, decode_32, encode};

    #[test]
    fn encode_matches_lowercase_hex() {
        assert_eq!(encode(&[0xAB, 0x01, 0xFF]), "ab01ff");
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn decode_round_trips_through_encode() {
        let bytes = [0x11_u8; 32];
        assert_eq!(decode_32(&encode(&bytes)), Some(bytes));
    }

    #[test]
    fn decode_32_rejects_the_wrong_length() {
        assert_eq!(decode_32("ab"), None);
        assert_eq!(decode_32(&"ab".repeat(33)), None);
    }

    #[test]
    fn decode_accepts_uppercase_hex_leniently_on_input() {
        // ADR-0046's "lowercase hex, no 0x prefix" rule governs this
        // crate's own *output*; accepting either case on *input* is a
        // deliberate leniency, not a contradiction of it.
        assert_eq!(decode_32(&"AB".repeat(32)), decode_32(&"ab".repeat(32)));
    }

    #[test]
    fn decode_32_rejects_non_hex_characters() {
        assert_eq!(decode_32(&"zz".repeat(32)), None);
    }

    #[test]
    fn decode_rejects_an_odd_length() {
        assert_eq!(decode("abc"), None);
    }

    #[test]
    fn decode_accepts_arbitrary_even_length() {
        assert_eq!(decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }
}
