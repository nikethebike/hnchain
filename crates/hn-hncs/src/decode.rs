use crate::{HncsError, HncsResult};

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
        match self.read_u8()? {
            0x00 => Ok(false),
            0x01 => Ok(true),
            value => Err(HncsError::InvalidBool { value }),
        }
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

        if length > max_len {
            return Err(HncsError::LengthLimitExceeded {
                length,
                max: max_len,
            });
        }

        self.read_exact(length)
    }

    /// Reads a bounded UTF-8 string encoded as `u32_length || utf8_bytes`.
    pub fn read_string(&mut self, max_len: usize) -> HncsResult<&'a str> {
        let bytes = self.read_bytes(max_len)?;
        core::str::from_utf8(bytes).map_err(|_| HncsError::InvalidUtf8)
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
}
