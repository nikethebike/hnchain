use crate::{
    HncsError, HncsResult, validate_bool_byte, validate_count, validate_length,
    validate_presence_byte, validate_string_bytes,
};

/// HNCS decoder for a single input byte slice.
#[derive(Debug)]
pub struct Decoder<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    /// Creates a decoder over an input byte slice.
    #[must_use]
    pub const fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    /// Returns the number of unread bytes.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.input.len() - self.offset
    }

    /// Returns true when the decoder consumed the full input.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.remaining() == 0
    }

    /// Rejects trailing bytes after a complete top-level value.
    pub const fn finish(&self) -> HncsResult<()> {
        if self.is_finished() {
            return Ok(());
        }

        Err(HncsError::TrailingBytes {
            remaining: self.remaining(),
        })
    }

    /// Reads a canonical HNCS boolean.
    pub fn read_bool(&mut self) -> HncsResult<bool> {
        let value = self.read_u8()?;
        validate_bool_byte(value)?;
        Ok(value == 0x01)
    }

    /// Reads an unsigned 8-bit integer.
    pub fn read_u8(&mut self) -> HncsResult<u8> {
        Ok(self.read_exact(1)?[0])
    }

    /// Reads an unsigned 16-bit integer using little-endian byte order.
    pub fn read_u16(&mut self) -> HncsResult<u16> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    /// Reads an unsigned 32-bit integer using little-endian byte order.
    pub fn read_u32(&mut self) -> HncsResult<u32> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    /// Reads an unsigned 64-bit integer using little-endian byte order.
    pub fn read_u64(&mut self) -> HncsResult<u64> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    /// Reads an unsigned 128-bit integer using little-endian byte order.
    pub fn read_u128(&mut self) -> HncsResult<u128> {
        Ok(u128::from_le_bytes(self.read_array()?))
    }

    /// Reads a signed 8-bit integer.
    pub fn read_i8(&mut self) -> HncsResult<i8> {
        Ok(i8::from_le_bytes(self.read_array()?))
    }

    /// Reads a signed 16-bit integer using little-endian byte order.
    pub fn read_i16(&mut self) -> HncsResult<i16> {
        Ok(i16::from_le_bytes(self.read_array()?))
    }

    /// Reads a signed 32-bit integer using little-endian byte order.
    pub fn read_i32(&mut self) -> HncsResult<i32> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    /// Reads a signed 64-bit integer using little-endian byte order.
    pub fn read_i64(&mut self) -> HncsResult<i64> {
        Ok(i64::from_le_bytes(self.read_array()?))
    }

    /// Reads a signed 128-bit integer using little-endian byte order.
    pub fn read_i128(&mut self) -> HncsResult<i128> {
        Ok(i128::from_le_bytes(self.read_array()?))
    }

    /// Reads a bounded byte sequence encoded as `u32_length || bytes`.
    pub fn read_bytes(&mut self, max_len: usize) -> HncsResult<&'a [u8]> {
        let length = self.read_u32()? as usize;

        validate_length(length, max_len)?;

        self.read_exact(length)
    }

    /// Reads a bounded UTF-8 string encoded as `u32_length || utf8_bytes`.
    pub fn read_string(&mut self, max_len: usize) -> HncsResult<&'a str> {
        let bytes = self.read_bytes(max_len)?;
        validate_string_bytes(bytes, max_len)
    }

    /// Reads an optional value encoded as `presence || value_if_present`.
    pub fn read_optional<T>(
        &mut self,
        mut read_value: impl FnMut(&mut Self) -> HncsResult<T>,
    ) -> HncsResult<Option<T>> {
        let presence = self.read_u8()?;
        validate_presence_byte(presence)?;

        if presence == 0x00 {
            return Ok(None);
        }

        Ok(Some(read_value(self)?))
    }

    /// Reads a bounded list preserving encoded order.
    pub fn read_list<T>(
        &mut self,
        max_count: usize,
        mut read_element: impl FnMut(&mut Self) -> HncsResult<T>,
    ) -> HncsResult<Vec<T>> {
        let count = self.read_count(max_count)?;
        let mut values = Vec::with_capacity(count);

        for _ in 0..count {
            values.push(read_element(self)?);
        }

        Ok(values)
    }

    /// Reads a bounded set and rejects unsorted or duplicate element encodings.
    pub fn read_set<T>(
        &mut self,
        max_count: usize,
        mut read_element: impl FnMut(&mut Self) -> HncsResult<T>,
    ) -> HncsResult<Vec<T>> {
        let count = self.read_count(max_count)?;
        let mut values = Vec::with_capacity(count);
        let mut previous: Option<&'a [u8]> = None;

        for _ in 0..count {
            let start = self.offset;
            let value = read_element(self)?;
            let encoded = &self.input[start..self.offset];

            if let Some(previous) = previous {
                match previous.cmp(encoded) {
                    core::cmp::Ordering::Greater => return Err(HncsError::UnsortedSet),
                    core::cmp::Ordering::Equal => return Err(HncsError::DuplicateSetElement),
                    core::cmp::Ordering::Less => {}
                }
            }

            previous = Some(encoded);
            values.push(value);
        }

        Ok(values)
    }

    /// Reads a bounded map and rejects unsorted or duplicate key encodings.
    pub fn read_map<K, V>(
        &mut self,
        max_count: usize,
        mut read_key: impl FnMut(&mut Self) -> HncsResult<K>,
        mut read_value: impl FnMut(&mut Self) -> HncsResult<V>,
    ) -> HncsResult<Vec<(K, V)>> {
        let count = self.read_count(max_count)?;
        let mut values = Vec::with_capacity(count);
        let mut previous_key: Option<&'a [u8]> = None;

        for _ in 0..count {
            let key_start = self.offset;
            let key = read_key(self)?;
            let encoded_key = &self.input[key_start..self.offset];

            if let Some(previous_key) = previous_key {
                match previous_key.cmp(encoded_key) {
                    core::cmp::Ordering::Greater => return Err(HncsError::UnsortedMap),
                    core::cmp::Ordering::Equal => return Err(HncsError::DuplicateMapKey),
                    core::cmp::Ordering::Less => {}
                }
            }

            previous_key = Some(encoded_key);
            let value = read_value(self)?;
            values.push((key, value));
        }

        Ok(values)
    }

    fn read_count(&mut self, max_count: usize) -> HncsResult<usize> {
        let count = self.read_u32()? as usize;
        validate_count(count, max_count)?;
        Ok(count)
    }

    fn read_array<const N: usize>(&mut self) -> HncsResult<[u8; N]> {
        let bytes = self.read_exact(N)?;
        let mut array = [0_u8; N];
        array.copy_from_slice(bytes);
        Ok(array)
    }

    fn read_exact(&mut self, length: usize) -> HncsResult<&'a [u8]> {
        let remaining = self.remaining();
        if remaining < length {
            return Err(HncsError::UnexpectedEof {
                needed: length,
                remaining,
            });
        }

        let start = self.offset;
        let end = start + length;
        self.offset = end;
        Ok(&self.input[start..end])
    }
}

#[cfg(test)]
mod tests {
    use super::Decoder;
    use crate::HncsError;

    #[test]
    fn reads_booleans() {
        let mut decoder = Decoder::new(&[0x00, 0x01]);

        assert_eq!(decoder.read_bool(), Ok(false));
        assert_eq!(decoder.read_bool(), Ok(true));
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn rejects_invalid_boolean_byte() {
        let mut decoder = Decoder::new(&[0x02]);

        assert_eq!(
            decoder.read_bool(),
            Err(HncsError::InvalidBool { value: 0x02 })
        );
    }

    #[test]
    fn reads_unsigned_integers_little_endian() {
        let input = [
            0xab, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12, 0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23,
            0x01, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0xef, 0xcd, 0xab, 0x89, 0x67,
            0x45, 0x23, 0x01,
        ];
        let mut decoder = Decoder::new(&input);

        assert_eq!(decoder.read_u8(), Ok(0xab));
        assert_eq!(decoder.read_u16(), Ok(0x1234));
        assert_eq!(decoder.read_u32(), Ok(0x1234_5678));
        assert_eq!(decoder.read_u64(), Ok(0x0123_4567_89ab_cdef));
        assert_eq!(
            decoder.read_u128(),
            Ok(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210)
        );
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn reads_signed_integers_little_endian() {
        let mut input = Vec::new();
        input.extend_from_slice(&(-1_i8).to_le_bytes());
        input.extend_from_slice(&(-2_i16).to_le_bytes());
        input.extend_from_slice(&(-3_i32).to_le_bytes());
        input.extend_from_slice(&(-4_i64).to_le_bytes());
        input.extend_from_slice(&(-5_i128).to_le_bytes());
        let mut decoder = Decoder::new(&input);

        assert_eq!(decoder.read_i8(), Ok(-1));
        assert_eq!(decoder.read_i16(), Ok(-2));
        assert_eq!(decoder.read_i32(), Ok(-3));
        assert_eq!(decoder.read_i64(), Ok(-4));
        assert_eq!(decoder.read_i128(), Ok(-5));
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn reads_bounded_bytes() {
        let mut decoder = Decoder::new(&[3, 0, 0, 0, b'a', b'b', b'c']);

        assert_eq!(decoder.read_bytes(3), Ok(&b"abc"[..]));
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn reads_empty_bytes() {
        let mut decoder = Decoder::new(&[0, 0, 0, 0]);

        assert_eq!(decoder.read_bytes(0), Ok(&b""[..]));
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn rejects_too_long_bytes_before_payload_read() {
        let mut decoder = Decoder::new(&[4, 0, 0, 0, b'a', b'b', b'c', b'd']);

        assert_eq!(
            decoder.read_bytes(3),
            Err(HncsError::LengthLimitExceeded { length: 4, max: 3 })
        );
        assert_eq!(decoder.remaining(), 4);
    }

    #[test]
    fn reads_bounded_string() {
        let mut decoder = Decoder::new(&[2, 0, 0, 0, b'h', b'n']);

        assert_eq!(decoder.read_string(2), Ok("hn"));
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn reads_empty_string() {
        let mut decoder = Decoder::new(&[0, 0, 0, 0]);

        assert_eq!(decoder.read_string(0), Ok(""));
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn preserves_distinct_unicode_representations() {
        let mut precomposed = Decoder::new(&[2, 0, 0, 0, 0xc3, 0xa9]);
        let mut decomposed = Decoder::new(&[3, 0, 0, 0, 0x65, 0xcc, 0x81]);

        assert_eq!(precomposed.read_string(2), Ok("\u{00e9}"));
        assert_eq!(decomposed.read_string(3), Ok("e\u{0301}"));
    }

    #[test]
    fn rejects_invalid_utf8_string() {
        let mut decoder = Decoder::new(&[1, 0, 0, 0, 0xff]);

        assert_eq!(decoder.read_string(1), Err(HncsError::InvalidUtf8));
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut decoder = Decoder::new(&[0x01, 0xff]);

        assert_eq!(decoder.read_bool(), Ok(true));
        assert_eq!(
            decoder.finish(),
            Err(HncsError::TrailingBytes { remaining: 1 })
        );
    }

    #[test]
    fn rejects_unexpected_eof() {
        let mut decoder = Decoder::new(&[0x01]);

        assert_eq!(
            decoder.read_u16(),
            Err(HncsError::UnexpectedEof {
                needed: 2,
                remaining: 1
            })
        );
    }

    #[test]
    fn reads_optional_values() {
        let mut none = Decoder::new(&[0x00]);
        let mut some = Decoder::new(&[0x01, 0x07]);

        assert_eq!(none.read_optional(read_u8_value), Ok(None));
        assert_eq!(some.read_optional(read_u8_value), Ok(Some(7)));
    }

    #[test]
    fn rejects_invalid_presence_byte() {
        let mut decoder = Decoder::new(&[0x02]);

        assert_eq!(
            decoder.read_optional(read_u8_value),
            Err(HncsError::InvalidPresence { value: 0x02 })
        );
    }

    #[test]
    fn reads_list_order_as_semantic() {
        let mut ordered = Decoder::new(&[2, 0, 0, 0, 1, 2]);
        let mut reversed = Decoder::new(&[2, 0, 0, 0, 2, 1]);

        assert_eq!(ordered.read_list(2, read_u8_value), Ok(vec![1, 2]));
        assert_eq!(reversed.read_list(2, read_u8_value), Ok(vec![2, 1]));
    }

    #[test]
    fn reads_sorted_sets() {
        let mut decoder = Decoder::new(&[3, 0, 0, 0, 1, 2, 3]);

        assert_eq!(decoder.read_set(3, read_u8_value), Ok(vec![1, 2, 3]));
    }

    #[test]
    fn rejects_unsorted_sets() {
        let mut decoder = Decoder::new(&[3, 0, 0, 0, 3, 1, 2]);

        assert_eq!(
            decoder.read_set(3, read_u8_value),
            Err(HncsError::UnsortedSet)
        );
    }

    #[test]
    fn rejects_duplicate_set_elements() {
        let mut decoder = Decoder::new(&[2, 0, 0, 0, 1, 1]);

        assert_eq!(
            decoder.read_set(2, read_u8_value),
            Err(HncsError::DuplicateSetElement)
        );
    }

    #[test]
    fn reads_sorted_maps() {
        let mut decoder = Decoder::new(&[2, 0, 0, 0, 1, 10, 2, 20]);

        assert_eq!(
            decoder.read_map(2, read_u8_value, read_u8_value),
            Ok(vec![(1, 10), (2, 20)])
        );
    }

    #[test]
    fn rejects_unsorted_maps() {
        let mut decoder = Decoder::new(&[2, 0, 0, 0, 2, 20, 1, 10]);

        assert_eq!(
            decoder.read_map(2, read_u8_value, read_u8_value),
            Err(HncsError::UnsortedMap)
        );
    }

    #[test]
    fn rejects_duplicate_map_keys() {
        let mut decoder = Decoder::new(&[2, 0, 0, 0, 1, 1, 1, 2]);

        assert_eq!(
            decoder.read_map(2, read_u8_value, read_u8_value),
            Err(HncsError::DuplicateMapKey)
        );
    }

    fn read_u8_value(decoder: &mut Decoder<'_>) -> crate::HncsResult<u8> {
        decoder.read_u8()
    }
}
