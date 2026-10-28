use std::fmt;

/// Errors raised while decoding, validating, adding to or merging sketches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The bytes are not a well-formed `AggregatorStateProto`.
    Proto(&'static str),
    /// The proto is well formed but describes a state this library refuses (wrong type,
    /// encoding version, precision, data length, ...).
    InvalidState(String),
    /// The two sketches have precisions that cannot be merged (neither is uniformly lower).
    IncompatiblePrecision {
        /// Normal and sparse precision of the receiving sketch.
        this: (i32, i32),
        /// Normal and sparse precision of the other sketch.
        other: (i32, i32),
    },
    /// The value type of the sketch does not allow this operation.
    IncompatibleType(String),
    /// The sparse difference-encoded stream is corrupt (truncated or overlong varint,
    /// negative values).
    CorruptSparseData(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Proto(m) => write!(f, "malformed proto: {m}"),
            Error::InvalidState(m) => write!(f, "invalid HLL++ state: {m}"),
            Error::IncompatiblePrecision { this, other } => write!(
                f,
                "precisions (p={}, sp={}) are not compatible to (p={}, sp={})",
                this.0, this.1, other.0, other.1
            ),
            Error::IncompatibleType(m) => write!(f, "incompatible value type: {m}"),
            Error::CorruptSparseData(m) => write!(f, "corrupt sparse data: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
