//! Error categories shared by the Core semantic layers.

use core::fmt;

/// Result type used by `xiaomu-core` APIs.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced when caller input cannot satisfy Core invariants.
///
/// Variants intentionally describe semantic categories rather than internal
/// storage details. More precise context can be added as the P0 value types
/// become concrete.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A text coordinate does not point to a valid UTF-8 character boundary.
    InvalidTextBoundary { offset: usize },
    /// A text coordinate or range exceeds the target text length.
    TextOutOfBounds { offset: usize, len: usize },
    /// A referenced document node does not exist in the current snapshot.
    UnknownNode,
    /// The requested selection is not valid for the current snapshot.
    InvalidSelection,
    /// A document does not satisfy structural or content invariants.
    InvalidDocument,
    /// A transaction step cannot be applied to the current snapshot.
    InvalidTransaction,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTextBoundary { offset } => {
                write!(f, "text offset {offset} is not a UTF-8 character boundary")
            }
            Self::TextOutOfBounds { offset, len } => {
                write!(f, "text offset {offset} is out of bounds for length {len}")
            }
            Self::UnknownNode => f.write_str("document node does not exist"),
            Self::InvalidSelection => f.write_str("selection is invalid for the document"),
            Self::InvalidDocument => f.write_str("document invariants are not satisfied"),
            Self::InvalidTransaction => f.write_str("transaction cannot be applied"),
        }
    }
}

impl std::error::Error for Error {}
