use crate::{HncsError, HncsResult};

/// Validates that a byte is a canonical HNCS boolean encoding.
pub const fn validate_bool_byte(value: u8) -> HncsResult<()> {
    match value {
        0x00 | 0x01 => Ok(()),
        value => Err(HncsError::InvalidBool { value }),
    }
}

/// Validates that a variable-length value fits the schema-defined HNCS limit.
pub fn validate_length(length: usize, max_len: usize) -> HncsResult<()> {
    if length > max_len {
        return Err(HncsError::LengthLimitExceeded {
            length,
            max: max_len,
        });
    }

    if length > u32::MAX as usize {
        return Err(HncsError::LengthFieldOverflow { length });
    }

    Ok(())
}

/// Validates that bytes are a bounded UTF-8 HNCS string payload.
pub fn validate_string_bytes(bytes: &[u8], max_len: usize) -> HncsResult<&str> {
    validate_length(bytes.len(), max_len)?;
    core::str::from_utf8(bytes).map_err(|_| HncsError::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::{validate_bool_byte, validate_length, validate_string_bytes};
    use crate::HncsError;

    #[test]
    fn validates_bool_bytes() {
        assert_eq!(validate_bool_byte(0x00), Ok(()));
        assert_eq!(validate_bool_byte(0x01), Ok(()));
        assert_eq!(
            validate_bool_byte(0x02),
            Err(HncsError::InvalidBool { value: 0x02 })
        );
    }

    #[test]
    fn validates_bounded_lengths() {
        assert_eq!(validate_length(0, 0), Ok(()));
        assert_eq!(validate_length(3, 3), Ok(()));
        assert_eq!(
            validate_length(4, 3),
            Err(HncsError::LengthLimitExceeded { length: 4, max: 3 })
        );
    }

    #[test]
    fn validates_string_bytes() {
        assert_eq!(validate_string_bytes(b"", 0), Ok(""));
        assert_eq!(validate_string_bytes("hns".as_bytes(), 3), Ok("hns"));
        assert_eq!(
            validate_string_bytes(&[0xff], 1),
            Err(HncsError::InvalidUtf8)
        );
    }
}
