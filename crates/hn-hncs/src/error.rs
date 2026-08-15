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
    /// An optional presence byte was not a canonical HNCS optional marker.
    InvalidPresence {
        /// Invalid encoded optional presence byte.
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
    /// A collection count exceeds the schema-defined limit.
    CountLimitExceeded {
        /// Encoded or requested count.
        count: usize,
        /// Maximum count allowed by the schema.
        max: usize,
    },
    /// A host memory length cannot be represented by the HNCS u32 length field.
    LengthFieldOverflow {
        /// Host memory length.
        length: usize,
    },
    /// A decoded string is not valid UTF-8.
    InvalidUtf8,
    /// A decoded set is not sorted by canonical element encoding.
    UnsortedSet,
    /// A decoded set contains duplicate canonical element encodings.
    DuplicateSetElement,
    /// A decoded map is not sorted by canonical key encoding.
    UnsortedMap,
    /// A decoded map contains duplicate canonical key encodings.
    DuplicateMapKey,
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
            Self::InvalidPresence { value } => {
                write!(formatter, "invalid HNCS optional presence byte {value}")
            }
            Self::UnexpectedEof { needed, remaining } => write!(
                formatter,
                "unexpected end of input: needed {needed} bytes, remaining {remaining}"
            ),
            Self::LengthLimitExceeded { length, max } => {
                write!(formatter, "length {length} exceeds maximum {max}")
            }
            Self::CountLimitExceeded { count, max } => {
                write!(formatter, "count {count} exceeds maximum {max}")
            }
            Self::LengthFieldOverflow { length } => {
                write!(
                    formatter,
                    "length {length} cannot fit into HNCS u32 length field"
                )
            }
            Self::InvalidUtf8 => formatter.write_str("invalid UTF-8 string"),
            Self::UnsortedSet => formatter.write_str("set elements are not sorted"),
            Self::DuplicateSetElement => formatter.write_str("set contains duplicate elements"),
            Self::UnsortedMap => formatter.write_str("map keys are not sorted"),
            Self::DuplicateMapKey => formatter.write_str("map contains duplicate keys"),
            Self::TrailingBytes { remaining } => {
                write!(formatter, "input has {remaining} trailing bytes")
            }
        }
    }
}

impl std::error::Error for HncsError {}
