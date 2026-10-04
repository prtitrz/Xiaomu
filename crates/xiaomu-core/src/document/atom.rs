//! Canonical inline-atom identity-independent values and placements.

use crate::text::TextOffset;
use crate::{Error, Result};

use super::{MarkSet, NodeId};

/// Typed semantic identity of an inline atom kind.
///
/// Built-in kinds and extension keys are distinct canonical values, even when
/// their string labels match. Core never infers built-in semantics from an
/// extension key or from an atom's fallback text.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AtomKind(AtomKindValue);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum AtomKindValue {
    Extension(String),
    HardBreak,
}

impl AtomKind {
    /// Creates an extension kind from a non-empty stable key.
    ///
    /// The original key is preserved exactly. This never creates a built-in
    /// kind, including when `key` is `"hardBreak"`.
    pub fn new(key: impl Into<String>) -> Result<Self> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(Error::InvalidAtomKind);
        }
        Ok(Self(AtomKindValue::Extension(key)))
    }

    /// Returns the built-in hard-break kind.
    ///
    /// A node with this kind must have empty attributes and an LF fallback.
    /// It uses the ordinary atom identity, placement and ordinal coordinates;
    /// it neither consumes a text byte nor converts existing literal LF text.
    #[must_use]
    pub const fn hard_break() -> Self {
        Self(AtomKindValue::HardBreak)
    }

    /// Returns whether this is the typed built-in hard break.
    #[must_use]
    pub const fn is_hard_break(&self) -> bool {
        matches!(self.0, AtomKindValue::HardBreak)
    }

    /// Returns the exact extension key, or `"hardBreak"` for the built-in.
    ///
    /// This label is not a lossless serialization of a typed kind. In
    /// particular, passing a built-in label to [`Self::new`] yields an
    /// extension, not the original kind. Codecs must use
    /// [`Self::is_hard_break`] and a wire format with a separate built-in tag;
    /// legacy extension-only formats must reject built-ins.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            AtomKindValue::Extension(key) => key,
            AtomKindValue::HardBreak => "hardBreak",
        }
    }
}

/// Canonical payload owned by an inline-atom node itself.
///
/// Extension-specific structured payload belongs in the node's [`NodeAttrs`]
/// (`crate::document::NodeAttrs`). `fallback_text` is promoted to a typed
/// field because clipboard, accessibility, and missing-renderer behavior all
/// require the same host-neutral textual fallback. Marks are independent of
/// surrounding text-run marks, retain exact typed attributes, and participate
/// in canonical equality and inverse transactions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineAtomContent {
    fallback_text: String,
    marks: MarkSet,
}

impl InlineAtomContent {
    /// Creates inline-atom content with a non-empty textual fallback and no marks.
    ///
    /// Even an LF fallback does not imply a hard-break kind. Kind-specific
    /// validation runs when the content is attached to a node.
    pub fn new(fallback_text: impl Into<String>) -> Result<Self> {
        let fallback_text = fallback_text.into();
        if fallback_text.is_empty() {
            return Err(Error::InvalidAtomFallback);
        }
        Ok(Self {
            fallback_text,
            marks: MarkSet::empty(),
        })
    }

    /// Creates an unmarked hard-break payload with its fixed LF fallback.
    ///
    /// Pair with [`AtomKind::hard_break`] and empty node attributes. Use
    /// [`Self::with_marks`] to retain the break's own imported formatting.
    #[must_use]
    pub fn hard_break() -> Self {
        Self {
            fallback_text: "\n".into(),
            marks: MarkSet::empty(),
        }
    }

    /// Replaces this atom's independent canonical marks.
    ///
    /// Core preserves the supplied normalized set without imposing host
    /// exclusion policies, including policies involving the Code mark.
    #[must_use]
    pub fn with_marks(mut self, marks: MarkSet) -> Self {
        self.marks = marks;
        self
    }

    /// Returns the canonical plain-text/accessibility fallback.
    #[must_use]
    pub fn fallback_text(&self) -> &str {
        &self.fallback_text
    }

    /// Returns this atom's own normalized marks, independent of adjacent text.
    #[must_use]
    pub const fn marks(&self) -> &MarkSet {
        &self.marks
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
