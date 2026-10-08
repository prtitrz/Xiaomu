//! Read-only textblock values and exact source mapping segments.

use std::ops::Range;

use xiaomu_core::{
    document::{AtomKind, NodeId, NodeKind},
    selection::InlinePoint,
    text::TextBuffer,
};

/// Which source side to choose when omitted atoms share a projected boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundarySide {
    /// The earliest source boundary, before any omitted atoms at this offset.
    BeforeAtoms,
    /// The latest source boundary, after any omitted atoms at this offset.
    AfterAtoms,
}

/// Meaning of a source-mapped segment. Atom fallbacks are never ordinary text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadingSpanKind {
    /// Unmodified canonical text, possibly spanning multiple formatting runs.
    Text,
    /// One real inline atom, even when its projection is zero bytes long.
    InlineAtom {
        /// Identity of the canonical atom node.
        node_id: NodeId,
        /// Typed built-in or extension kind; no inference from fallback text.
        kind: AtomKind,
    },
}

/// One mapping segment in an immutable textblock projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingSpan {
    pub(super) projected: Range<usize>,
    pub(super) start: InlinePoint,
    pub(super) end: InlinePoint,
    pub(super) kind: ReadingSpanKind,
}

impl ReadingSpan {
    /// Half-open UTF-8 byte range in [`ReadingTextBlock::text`].
    /// Omitted atoms retain an empty range here and a nonempty source range.
    #[must_use]
    pub fn projected_range(&self) -> Range<usize> {
        self.projected.clone()
    }

    /// Exact source boundary before this segment.
    #[must_use]
    pub const fn start(&self) -> InlinePoint {
        self.start
    }

    /// Exact source boundary after this segment.
    #[must_use]
    pub const fn end(&self) -> InlinePoint {
        self.end
    }

    /// Returns the segment's canonical semantic category.
    #[must_use]
    pub const fn kind(&self) -> &ReadingSpanKind {
        &self.kind
    }
}

/// One inline-bearing node in depth-first canonical child order.
///
/// Containers add no text and table cells occur once in structural origin-cell
/// order. Empty textblocks are retained. A heading is identified by its exact
/// [`NodeKind::Heading`] level; no trimming or outline hierarchy is imposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingTextBlock {
    pub(super) node_id: NodeId,
    pub(super) order: usize,
    pub(super) kind: NodeKind,
    pub(super) text: String,
    pub(super) canonical: TextBuffer,
    pub(super) spans: Vec<ReadingSpan>,
}

impl ReadingTextBlock {
    /// Canonical inline-bearing node identity.
    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Zero-based index in [`super::ReadingProjection::blocks`].
    #[must_use]
    pub const fn order(&self) -> usize {
        self.order
    }

    /// Original block kind, including typed heading level when applicable.
    #[must_use]
    pub const fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// Text with the explicitly chosen atom projection; no normalization.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Ordered source-mapping segments, retaining zero-width omitted atoms.
    #[must_use]
    pub fn spans(&self) -> &[ReadingSpan] {
        &self.spans
    }

    /// Original canonical text fragments, excluding all atom representations.
    /// Their concatenation is the text-only content, independent of options.
    pub fn text_fragments(&self) -> impl Iterator<Item = &str> {
        self.spans
            .iter()
            .filter(|span| matches!(span.kind, ReadingSpanKind::Text))
            .map(|span| &self.text[span.projected.clone()])
    }
}
