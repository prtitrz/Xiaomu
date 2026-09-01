//! Canonical inline-atom identity-independent values and placements.

use crate::text::TextOffset;
use crate::{Error, Result};

use super::NodeId;

/// Stable semantic key for an extension-defined inline atom kind.
///
/// The key is canonical document data. Renderer registration and host
/// capability lookup may use it, but Core never interprets product-specific
/// meaning from the string.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AtomKind(String);

impl AtomKind {
    /// Creates an atom kind from a non-empty stable key.
    pub fn new(key: impl Into<String>) -> Result<Self> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(Error::InvalidAtomKind);
        }
        Ok(Self(key))
    }

    /// Returns the stable extension key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonical payload owned by an inline-atom node itself.
///
/// Extension-specific structured payload belongs in the node's [`NodeAttrs`]
/// (`crate::document::NodeAttrs`). `fallback_text` is promoted to a typed
/// field because clipboard, accessibility, and missing-renderer behavior all
/// require the same host-neutral textual fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineAtomContent {
    fallback_text: String,
}

impl InlineAtomContent {
    /// Creates inline-atom content with a non-empty textual fallback.
    pub fn new(fallback_text: impl Into<String>) -> Result<Self> {
        let fallback_text = fallback_text.into();
        if fallback_text.is_empty() {
            return Err(Error::InvalidAtomFallback);
        }
        Ok(Self { fallback_text })
    }

    /// Returns the canonical plain-text/accessibility fallback.
    #[must_use]
    pub fn fallback_text(&self) -> &str {
        &self.fallback_text
    }
}

/// One ordered reference to an inline-atom node from an [`InlineContent`]
/// (`crate::document::InlineContent`).
///
/// `text_offset` remains a UTF-8 byte coordinate in the surrounding text.
/// Multiple placements may share the same offset; their order in the
/// normalized placement vector defines atom ordinal at that boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InlineAtomPlacement {
    atom: NodeId,
    text_offset: TextOffset,
}

impl InlineAtomPlacement {
    /// Creates a placement. The surrounding [`InlineContent`]
    /// (`crate::document::InlineContent`) validates the offset against its
    /// normalized text when the placement is attached.
    #[must_use]
    pub const fn new(atom: NodeId, text_offset: TextOffset) -> Self {
        Self { atom, text_offset }
    }

    /// Returns the referenced canonical atom node.
    #[must_use]
    pub const fn atom(self) -> NodeId {
        self.atom
    }

    /// Returns the UTF-8 text boundary anchoring this atom.
    #[must_use]
    pub const fn text_offset(self) -> TextOffset {
        self.text_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::NodeStoreBuilder;

    #[test]
    fn atom_kind_rejects_empty_keys() {
        assert_eq!(AtomKind::new(""), Err(Error::InvalidAtomKind));
        assert_eq!(AtomKind::new("   "), Err(Error::InvalidAtomKind));
        assert_eq!(AtomKind::new("mention").unwrap().as_str(), "mention");
    }

    #[test]
    fn atom_content_requires_a_fallback() {
        assert_eq!(InlineAtomContent::new(""), Err(Error::InvalidAtomFallback));
        assert_eq!(
            InlineAtomContent::new("@Alice").unwrap().fallback_text(),
            "@Alice"
        );
    }

    #[test]
    fn placement_keeps_identity_and_text_boundary_separate() {
        let builder = NodeStoreBuilder::new();
        let atom = builder.peek_next_id();
        let offset = TextOffset::ZERO;
        let placement = InlineAtomPlacement::new(atom, offset);

        assert_eq!(placement.atom(), atom);
        assert_eq!(placement.text_offset(), offset);
    }
}
