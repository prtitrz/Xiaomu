//! Built-in canonical node kinds.

use crate::{Error, Result};

use super::AtomKind;

/// Valid heading level for a canonical heading node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HeadingLevel(u8);

impl HeadingLevel {
    /// Creates a heading level in the inclusive range `1..=6`.
    pub fn new(level: u8) -> Result<Self> {
        if (1..=6).contains(&level) {
            Ok(Self(level))
        } else {
            Err(Error::InvalidHeadingLevel { level })
        }
    }

    /// Returns the numeric heading level.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

/// Semantic kind of a canonical document node.
///
/// Content-shape validation belongs to the node/document layer introduced in
/// P0.2B. This enum identifies semantics without exposing storage layout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NodeKind {
    /// Root document container.
    Document,
    /// Paragraph with inline content.
    Paragraph,
    /// Heading with a validated level.
    Heading(HeadingLevel),
    /// Block quote container.
    Quote,
    /// Unordered list container.
    BulletList,
    /// Ordered list container.
    OrderedList,
    /// List item container.
    ListItem,
    /// Code block.
    CodeBlock,
    /// Horizontal rule atomic block.
    HorizontalRule,
    /// Image atomic block.
    Image,
    /// Extension-defined inline atom with a stable semantic key.
    InlineAtom(AtomKind),
    /// Extension-defined block kind preserved by its stable key.
    Custom(String),
}

impl NodeKind {
    /// Creates an extension-defined node kind from a non-empty stable key.
    pub fn custom(key: impl Into<String>) -> Result<Self> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(Error::InvalidCustomNodeKind);
        }
        Ok(Self::Custom(key))
    }
}
