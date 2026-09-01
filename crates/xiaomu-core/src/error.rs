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
    InvalidTextBoundary {
        /// The offending UTF-8 byte offset.
        offset: usize,
    },
    /// A text coordinate or range exceeds the target text length.
    TextOutOfBounds {
        /// The offending UTF-8 byte offset.
        offset: usize,
        /// The target text length in UTF-8 bytes.
        len: usize,
    },
    /// A text range has its start after its end.
    InvalidTextRange {
        /// The range start in UTF-8 bytes.
        start: usize,
        /// The range end in UTF-8 bytes.
        end: usize,
    },
    /// A heading level is outside the supported range `1..=6`.
    InvalidHeadingLevel {
        /// The offending heading level.
        level: u8,
    },
    /// An extension-defined node kind has an empty key.
    InvalidCustomNodeKind,
    /// An inline-atom semantic kind has an empty stable key.
    InvalidAtomKind,
    /// An inline atom has no textual fallback for clipboard/accessibility.
    InvalidAtomFallback,
    /// The same inline-atom node is referenced more than once by one parent.
    DuplicateInlineAtomReference,
    /// An inline placement references a node that is not an inline atom.
    InvalidInlineAtomReference,
    /// A node attribute key is empty after trimming.
    InvalidNodeAttrKey,
    /// A mark set contains conflicting values for one semantic mark kind.
    InvalidMarkSet,
    /// Canonical text runs must not persist empty text.
    EmptyTextRun,
    /// A node kind was paired with an incompatible content shape.
    InvalidNodeContent,
    /// A referenced document node does not exist in the current snapshot.
    UnknownNode,
    /// Node identity allocation exhausted its internal representation.
    NodeIdExhausted,
    /// One parent contains the same child reference more than once.
    DuplicateChildReference,
    /// A parent/child node-kind relationship violates document structure.
    InvalidChildKind,
    /// The selected root is missing or is not a document root.
    InvalidRootNode,
    /// The canonical node graph contains a cycle.
    CyclicDocument,
    /// A reachable node is referenced by more than one parent.
    MultipleNodeParents,
    /// The store contains a node that is not reachable from the document root.
    UnreachableNode,
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
            Self::InvalidTextRange { start, end } => {
                write!(f, "text range start {start} is after end {end}")
            }
            Self::InvalidHeadingLevel { level } => {
                write!(
                    f,
                    "heading level {level} is outside the supported range 1..=6"
                )
            }
            Self::InvalidCustomNodeKind => f.write_str("custom node kind key must not be empty"),
            Self::InvalidAtomKind => f.write_str("inline atom kind key must not be empty"),
            Self::InvalidAtomFallback => f.write_str("inline atom fallback text must not be empty"),
            Self::DuplicateInlineAtomReference => {
                f.write_str("inline content contains a duplicate atom reference")
            }
            Self::InvalidInlineAtomReference => {
                f.write_str("inline placement must reference an inline-atom node")
            }
            Self::InvalidNodeAttrKey => f.write_str("node attribute key must not be empty"),
            Self::InvalidMarkSet => f.write_str("mark set contains conflicting mark values"),
            Self::EmptyTextRun => f.write_str("canonical text run must not be empty"),
            Self::InvalidNodeContent => {
                f.write_str("node content shape is incompatible with its node kind")
            }
            Self::UnknownNode => f.write_str("document node does not exist"),
            Self::NodeIdExhausted => f.write_str("node identity space is exhausted"),
            Self::DuplicateChildReference => {
                f.write_str("parent contains a duplicate child reference")
            }
            Self::InvalidChildKind => f.write_str("parent does not accept this child node kind"),
            Self::InvalidRootNode => f.write_str("document root must be a document node"),
            Self::CyclicDocument => f.write_str("document node graph contains a cycle"),
            Self::MultipleNodeParents => f.write_str("document node has multiple parents"),
            Self::UnreachableNode => f.write_str("document store contains an unreachable node"),
            Self::InvalidSelection => f.write_str("selection is invalid for the document"),
            Self::InvalidDocument => f.write_str("document invariants are not satisfied"),
            Self::InvalidTransaction => f.write_str("transaction cannot be applied"),
        }
    }
}

impl std::error::Error for Error {}
