use core::fmt;

/// Result type used by HNChain primitive constructors and conversions.
pub type PrimitiveResult<T> = Result<T, PrimitiveError>;

/// Validation and conversion errors for HNChain core primitive types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrimitiveError {
    /// `chain_id = 0x00` was supplied; that value is reserved and never
    /// a valid chain identifier (ADR-0006, "Chain And Network Binding").
    ReservedChainId,
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
            Self::ReservedChainId => {
                formatter.write_str("chain_id 0x00 is reserved and never valid")
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
