use core::fmt;

/// Result type used by HNCS encoding and decoding operations.
pub type HncsResult<T> = Result<T, HncsError>;

/// HNCS encoding and decoding errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HncsError {
    /// A boolean byte was not a canonical HNCS boolean value.
    InvalidBool {
        /// Invalid encoded boolean byte.
        value: u8,
    },
    /// The input ended before the requested value could be decoded.
    UnexpectedEof {
        /// Number of bytes required by the attempted read.
        needed: usize,
        /// Number of bytes still available in the input.
        remaining: usize,
    },
    /// A variable-length value exceeds the schema-defined limit.
    LengthLimitExceeded {
        /// Encoded or requested length.
        length: usize,
        /// Maximum length allowed by the schema.
        max: usize,
    },
    /// A host memory length cannot be represented by the HNCS u32 length field.
    LengthFieldOverflow {
        /// Host memory length.
        length: usize,
    },
    /// A decoded string is not valid UTF-8.
    InvalidUtf8,
    /// Bytes remain after a complete top-level value has been decoded.
    TrailingBytes {
        /// Number of trailing bytes.
        remaining: usize,
    },
}

impl fmt::Display for HncsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBool { value } => {
                write!(formatter, "invalid HNCS boolean byte {value}")
            }
            Self::UnexpectedEof { needed, remaining } => write!(
                formatter,
                "unexpected end of input: needed {needed} bytes, remaining {remaining}"
            ),
            Self::LengthLimitExceeded { length, max } => {
                write!(formatter, "length {length} exceeds maximum {max}")
            }
            Self::LengthFieldOverflow { length } => {
                write!(
                    formatter,
                    "length {length} cannot fit into HNCS u32 length field"
                )
            }
            Self::InvalidUtf8 => formatter.write_str("invalid UTF-8 string"),
            Self::TrailingBytes { remaining } => {
                write!(formatter, "input has {remaining} trailing bytes")
            }
        }
    }
}

impl std::error::Error for HncsError {}
