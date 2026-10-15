//! Lowercase hex encoding for this binary's own text output.

/// Encodes `bytes` as lowercase hex, no `0x` prefix (ADR-0046's own
/// "Decided: Value Encoding Rules" convention, reused here since this
/// binary's own output should read the same way the RPC surface
/// already does).
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn encodes_lowercase_with_no_prefix() {
        assert_eq!(encode(&[0xAB, 0x01, 0xFF]), "ab01ff");
        assert_eq!(encode(&[]), "");
    }
}
