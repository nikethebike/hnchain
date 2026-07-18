use core::fmt;

/// Result type used by HNChain primitive constructors and conversions.
pub type PrimitiveResult<T> = Result<T, PrimitiveError>;

/// Validation and conversion errors for HNChain core primitive types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrimitiveError {
    /// The supplied chain identifier is empty.
    EmptyChainId,
    /// The supplied chain identifier exceeds the maximum encoded length.
    ChainIdTooLong {
        /// Actual byte length.
        actual: usize,
        /// Maximum allowed byte length.
        max: usize,
    },
    /// The supplied chain identifier does not start with a lowercase letter.
    InvalidChainIdStart {
        /// First invalid byte, if present.
        byte: Option<u8>,
    },
    /// The supplied chain identifier does not end with a lowercase letter or digit.
    InvalidChainIdEnd {
        /// Last invalid byte, if present.
        byte: Option<u8>,
    },
    /// The supplied chain identifier contains an invalid byte.
    InvalidChainIdByte {
        /// Byte position inside the identifier.
        index: usize,
        /// Invalid byte value.
        byte: u8,
    },
    /// The supplied chain identifier contains two adjacent hyphens.
    ConsecutiveHyphenInChainId {
        /// Position of the second hyphen.
        index: usize,
    },
    /// A host memory size cannot be represented as a protocol byte length.
    ByteLengthOverflow {
        /// Original host memory size.
        value: usize,
        /// Maximum protocol byte length.
        max: u32,
    },
    /// Checked arithmetic overflowed for a primitive type.
    ArithmeticOverflow {
        /// Name of the primitive type.
        type_name: &'static str,
    },
}

impl fmt::Display for PrimitiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyChainId => formatter.write_str("chain id must not be empty"),
            Self::ChainIdTooLong { actual, max } => write!(
                formatter,
                "chain id is {actual} bytes, which exceeds the maximum of {max} bytes"
            ),
            Self::InvalidChainIdStart { byte } => {
                write!(formatter, "chain id has invalid start byte {byte:?}")
            }
            Self::InvalidChainIdEnd { byte } => {
                write!(formatter, "chain id has invalid end byte {byte:?}")
            }
            Self::InvalidChainIdByte { index, byte } => write!(
                formatter,
                "chain id contains invalid byte {byte} at index {index}"
            ),
            Self::ConsecutiveHyphenInChainId { index } => {
                write!(
                    formatter,
                    "chain id contains consecutive hyphen at index {index}"
                )
            }
            Self::ByteLengthOverflow { value, max } => {
                write!(formatter, "byte length {value} exceeds maximum {max}")
            }
            Self::ArithmeticOverflow { type_name } => {
                write!(formatter, "arithmetic overflow for {type_name}")
            }
        }
    }
}

impl std::error::Error for PrimitiveError {}
