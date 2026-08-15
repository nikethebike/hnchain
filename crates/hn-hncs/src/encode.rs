use crate::{HncsError, HncsResult, validate_count, validate_length};

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

/// Writes an optional value as `presence || value_if_present`.
pub fn write_optional<T: ?Sized>(
    out: &mut Vec<u8>,
    value: Option<&T>,
    mut write_value: impl FnMut(&mut Vec<u8>, &T) -> HncsResult<()>,
) -> HncsResult<()> {
    match value {
        None => write_bool(out, false),
        Some(value) => {
            write_bool(out, true);
            write_value(out, value)?;
        }
    }

    Ok(())
}

/// Writes a bounded list as `u32_count || elements` preserving input order.
pub fn write_list<T>(
    out: &mut Vec<u8>,
    values: &[T],
    max_count: usize,
    mut write_element: impl FnMut(&mut Vec<u8>, &T) -> HncsResult<()>,
) -> HncsResult<()> {
    validate_count(values.len(), max_count)?;
    write_u32_count(out, values.len())?;

    for value in values {
        write_element(out, value)?;
    }

    Ok(())
}

/// Writes a bounded set sorted by canonical encoded element bytes.
pub fn write_set<T>(
    out: &mut Vec<u8>,
    values: &[T],
    max_count: usize,
    mut write_element: impl FnMut(&mut Vec<u8>, &T) -> HncsResult<()>,
) -> HncsResult<()> {
    validate_count(values.len(), max_count)?;

    let mut elements = Vec::with_capacity(values.len());
    for value in values {
        let mut encoded = Vec::new();
        write_element(&mut encoded, value)?;
        elements.push(encoded);
    }

    elements.sort();
    if elements.windows(2).any(|window| window[0] == window[1]) {
        return Err(HncsError::DuplicateSetElement);
    }

    write_u32_count(out, elements.len())?;
    for element in elements {
        out.extend_from_slice(&element);
    }

    Ok(())
}

/// Writes a bounded map sorted by canonical encoded key bytes.
pub fn write_map<K, V>(
    out: &mut Vec<u8>,
    entries: &[(K, V)],
    max_count: usize,
    mut write_key: impl FnMut(&mut Vec<u8>, &K) -> HncsResult<()>,
    mut write_value: impl FnMut(&mut Vec<u8>, &V) -> HncsResult<()>,
) -> HncsResult<()> {
    validate_count(entries.len(), max_count)?;

    let mut encoded_entries = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let mut encoded_key = Vec::new();
        let mut encoded_value = Vec::new();
        write_key(&mut encoded_key, key)?;
        write_value(&mut encoded_value, value)?;
        encoded_entries.push((encoded_key, encoded_value));
    }

    encoded_entries.sort_by(|left, right| left.0.cmp(&right.0));
    if encoded_entries
        .windows(2)
        .any(|window| window[0].0 == window[1].0)
    {
        return Err(HncsError::DuplicateMapKey);
    }

    write_u32_count(out, encoded_entries.len())?;
    for (key, value) in encoded_entries {
        out.extend_from_slice(&key);
        out.extend_from_slice(&value);
    }

    Ok(())
}

fn write_u32_count(out: &mut Vec<u8>, count: usize) -> HncsResult<()> {
    let encoded_count =
        u32::try_from(count).map_err(|_| HncsError::LengthFieldOverflow { length: count })?;
    write_u32(out, encoded_count);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        write_bool, write_bytes, write_i8, write_i16, write_i32, write_i64, write_i128, write_list,
        write_map, write_optional, write_set, write_string, write_u8, write_u16, write_u32,
        write_u64, write_u128,
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

    #[test]
    fn writes_optional_values() {
        let mut none = Vec::new();
        let mut some = Vec::new();

        assert_eq!(
            write_optional::<u8>(&mut none, None, write_u8_value),
            Ok(())
        );
        assert_eq!(
            write_optional(&mut some, Some(&7_u8), write_u8_value),
            Ok(())
        );

        assert_eq!(none, [0x00]);
        assert_eq!(some, [0x01, 0x07]);
    }

    #[test]
    fn writes_lists_in_input_order() {
        let mut ordered = Vec::new();
        let mut reversed = Vec::new();

        assert_eq!(
            write_list(&mut ordered, &[1_u8, 2_u8], 2, write_u8_value),
            Ok(())
        );
        assert_eq!(
            write_list(&mut reversed, &[2_u8, 1_u8], 2, write_u8_value),
            Ok(())
        );

        assert_eq!(ordered, [2, 0, 0, 0, 1, 2]);
        assert_eq!(reversed, [2, 0, 0, 0, 2, 1]);
        assert_ne!(ordered, reversed);
    }

    #[test]
    fn writes_sets_in_canonical_order() {
        let mut first = Vec::new();
        let mut second = Vec::new();

        assert_eq!(
            write_set(&mut first, &[3_u8, 1_u8, 2_u8], 3, write_u8_value),
            Ok(())
        );
        assert_eq!(
            write_set(&mut second, &[2_u8, 3_u8, 1_u8], 3, write_u8_value),
            Ok(())
        );

        assert_eq!(first, [3, 0, 0, 0, 1, 2, 3]);
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_duplicate_set_elements() {
        let mut out = Vec::new();

        assert_eq!(
            write_set(&mut out, &[1_u8, 1_u8], 2, write_u8_value),
            Err(HncsError::DuplicateSetElement)
        );
    }

    #[test]
    fn writes_maps_in_canonical_key_order() {
        let mut first = Vec::new();
        let mut second = Vec::new();

        assert_eq!(
            write_map(
                &mut first,
                &[(2_u8, 20_u8), (1_u8, 10_u8)],
                2,
                write_u8_value,
                write_u8_value
            ),
            Ok(())
        );
        assert_eq!(
            write_map(
                &mut second,
                &[(1_u8, 10_u8), (2_u8, 20_u8)],
                2,
                write_u8_value,
                write_u8_value
            ),
            Ok(())
        );

        assert_eq!(first, [2, 0, 0, 0, 1, 10, 2, 20]);
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_duplicate_map_keys() {
        let mut out = Vec::new();

        assert_eq!(
            write_map(
                &mut out,
                &[(1_u8, 1_u8), (1_u8, 2_u8)],
                2,
                write_u8_value,
                write_u8_value
            ),
            Err(HncsError::DuplicateMapKey)
        );
    }

    fn write_u8_value(out: &mut Vec<u8>, value: &u8) -> crate::HncsResult<()> {
        write_u8(out, *value);
        Ok(())
    }
}
