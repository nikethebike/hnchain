use crate::{HncsError, HncsResult, validate_length};

/// Writes a canonical HNCS boolean value.
pub fn write_bool(out: &mut Vec<u8>, value: bool) {
    out.push(u8::from(value));
}

/// Writes an unsigned 8-bit integer.
pub fn write_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

/// Writes an unsigned 16-bit integer using little-endian byte order.
pub fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes an unsigned 32-bit integer using little-endian byte order.
pub fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes an unsigned 64-bit integer using little-endian byte order.
pub fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes an unsigned 128-bit integer using little-endian byte order.
pub fn write_u128(out: &mut Vec<u8>, value: u128) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a signed 8-bit integer.
pub fn write_i8(out: &mut Vec<u8>, value: i8) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a signed 16-bit integer using little-endian byte order.
pub fn write_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a signed 32-bit integer using little-endian byte order.
pub fn write_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a signed 64-bit integer using little-endian byte order.
pub fn write_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a signed 128-bit integer using little-endian byte order.
pub fn write_i128(out: &mut Vec<u8>, value: i128) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a bounded byte sequence as `u32_length || bytes`.
pub fn write_bytes(out: &mut Vec<u8>, bytes: &[u8], max_len: usize) -> HncsResult<()> {
    let length = bytes.len();
    validate_length(length, max_len)?;
    let encoded_length =
        u32::try_from(length).map_err(|_| HncsError::LengthFieldOverflow { length })?;

    write_u32(out, encoded_length);
    out.extend_from_slice(bytes);
    Ok(())
}

/// Writes a bounded UTF-8 string as `u32_length || utf8_bytes`.
pub fn write_string(out: &mut Vec<u8>, value: &str, max_len: usize) -> HncsResult<()> {
    write_bytes(out, value.as_bytes(), max_len)
}

#[cfg(test)]
mod tests {
    use super::{
        write_bool, write_bytes, write_i8, write_i16, write_i32, write_i64, write_i128,
        write_string, write_u8, write_u16, write_u32, write_u64, write_u128,
    };
    use crate::HncsError;

    #[test]
    fn writes_booleans() {
        let mut out = Vec::new();

        write_bool(&mut out, false);
        write_bool(&mut out, true);

        assert_eq!(out, [0x00, 0x01]);
    }

    #[test]
    fn writes_unsigned_integers_little_endian() {
        let mut out = Vec::new();

        write_u8(&mut out, 0xab);
        write_u16(&mut out, 0x1234);
        write_u32(&mut out, 0x1234_5678);
        write_u64(&mut out, 0x0123_4567_89ab_cdef);
        write_u128(&mut out, 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210);

        assert_eq!(
            out,
            [
                0xab, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12, 0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23,
                0x01, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0xef, 0xcd, 0xab, 0x89, 0x67,
                0x45, 0x23, 0x01,
            ]
        );
    }

    #[test]
    fn writes_signed_integers_little_endian() {
        let mut out = Vec::new();

        write_i8(&mut out, -1);
        write_i16(&mut out, -2);
        write_i32(&mut out, -3);
        write_i64(&mut out, -4);
        write_i128(&mut out, -5);

        assert_eq!(out[0], 0xff);
        assert_eq!(&out[1..3], &(-2_i16).to_le_bytes());
        assert_eq!(&out[3..7], &(-3_i32).to_le_bytes());
        assert_eq!(&out[7..15], &(-4_i64).to_le_bytes());
        assert_eq!(&out[15..31], &(-5_i128).to_le_bytes());
    }

    #[test]
    fn writes_bounded_bytes() {
        let mut out = Vec::new();

        assert_eq!(write_bytes(&mut out, b"abc", 3), Ok(()));

        assert_eq!(out, [3, 0, 0, 0, b'a', b'b', b'c']);
    }

    #[test]
    fn writes_empty_bytes() {
        let mut out = Vec::new();

        assert_eq!(write_bytes(&mut out, b"", 0), Ok(()));

        assert_eq!(out, [0, 0, 0, 0]);
    }

    #[test]
    fn rejects_too_long_bytes() {
        let mut out = Vec::new();

        assert_eq!(
            write_bytes(&mut out, b"abcd", 3),
            Err(HncsError::LengthLimitExceeded { length: 4, max: 3 })
        );
        assert!(out.is_empty());
    }

    #[test]
    fn writes_bounded_string() {
        let mut out = Vec::new();

        assert_eq!(write_string(&mut out, "hn", 2), Ok(()));

        assert_eq!(out, [2, 0, 0, 0, b'h', b'n']);
    }

    #[test]
    fn writes_empty_string() {
        let mut out = Vec::new();

        assert_eq!(write_string(&mut out, "", 0), Ok(()));

        assert_eq!(out, [0, 0, 0, 0]);
    }

    #[test]
    fn does_not_normalize_unicode_strings() {
        let mut precomposed = Vec::new();
        let mut decomposed = Vec::new();

        assert_eq!(write_string(&mut precomposed, "\u{00e9}", 2), Ok(()));
        assert_eq!(write_string(&mut decomposed, "e\u{0301}", 3), Ok(()));

        assert_eq!(precomposed, [2, 0, 0, 0, 0xc3, 0xa9]);
        assert_eq!(decomposed, [3, 0, 0, 0, 0x65, 0xcc, 0x81]);
    }
}
