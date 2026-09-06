//! Baseline Markdown codec error model.

use core::fmt;

/// Result type used by the Markdown codec.
pub type Result<T> = core::result::Result<T, MarkdownCodecError>;

/// Errors raised when a document and the baseline Markdown contract disagree.
///
/// The baseline codec never silently drops content: every variant describes a
/// refusal, and the canonical document stays untouched behind the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MarkdownCodecError {
    /// A canonical node kind has no baseline Markdown representation.
    UnsupportedNodeKind {
        /// Stable description of the offending node kind.
        kind: String,
    },
    /// A node carries attributes outside the baseline Markdown contract.
    ///
    /// Unknown extension attrs are preserved in the canonical document; the
    /// codec refuses to export instead of dropping them.
    UnsupportedAttributes {
        /// Stable description of the offending node kind.
        kind: String,
        /// Attribute keys the baseline cannot represent, in key order.
        keys: Vec<String>,
    },
    /// An inline mark or mark combination has no baseline representation.
    UnsupportedMark {
        /// Stable description of the offending mark or combination.
        mark: String,
    },
    /// An Image node references a host asset instead of an external URL.
    ///
    /// Mapping host `AssetRef` values to Markdown URLs belongs to a host
    /// adapter policy, so the baseline refuses to invent one.
    AssetImageNotExportable,
    /// A paragraph carries no text, which Markdown cannot represent.
    EmptyParagraph,
    /// Leading, trailing, or blank-line whitespace inside one inline node
    /// cannot survive a Markdown round-trip.
    LossyWhitespace {
        /// Stable description of the offending node kind.
        kind: String,
    },
    /// A node kind whose Markdown form is a single line carries a hard break.
    HardBreakNotRepresentable {
        /// Stable description of the offending node kind.
        kind: String,
    },
    /// Markdown source violates the baseline grammar.
    InvalidMarkdown {
        /// One-based source line the violation was detected on.
        line: usize,
        /// Human-readable reason.
        reason: String,
    },
    /// The parsed tree failed canonical document validation.
    InvalidDocument(xiaomu_core::Error),
}

impl fmt::Display for MarkdownCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedNodeKind { kind } => {
                write!(f, "node kind has no baseline Markdown form: {kind}")
            }
            Self::UnsupportedAttributes { kind, keys } => write!(
                f,
                "{kind} carries attributes outside the baseline contract: "
            )
            .and_then(|()| write_attr_keys(f, keys)),
            Self::UnsupportedMark { mark } => {
                write!(f, "inline mark has no baseline Markdown form: {mark}")
            }
            Self::AssetImageNotExportable => write!(
                f,
                "image references a host asset; only external URLs are exportable"
            ),
            Self::EmptyParagraph => write!(f, "empty paragraph has no Markdown form"),
            Self::LossyWhitespace { kind } => {
                write!(
                    f,
                    "leading, trailing, or blank-line whitespace in {kind} cannot round-trip"
                )
            }
            Self::HardBreakNotRepresentable { kind } => {
                write!(f, "hard break has no {kind} Markdown form")
            }
            Self::InvalidMarkdown { line, reason } => {
                write!(f, "invalid Markdown at line {line}: {reason}")
            }
            Self::InvalidDocument(error) => write!(f, "parsed document is invalid: {error}"),
        }
    }
}

fn write_attr_keys(f: &mut fmt::Formatter<'_>, keys: &[String]) -> fmt::Result {
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{key}")?;
    }
    Ok(())
}

impl std::error::Error for MarkdownCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidDocument(error) => Some(error),
            _ => None,
        }
    }
}
