//! Typed text positions inside inline-bearing document nodes.

use crate::document::{NodeId, XiaomuDocument};
use crate::text::TextOffset;
use crate::{Error, Result};

use super::CursorAffinity;

/// A stable position inside the inline text content of one document node.
///
/// A `TextPoint` only becomes meaningful for a specific document snapshot:
/// the node must exist, carry inline content, and the offset must be a valid
/// UTF-8 scalar boundary of that node's concatenated text. Validation happens
/// through [`TextPoint::validate`]; stale points from earlier revisions must
/// be revalidated before use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextPoint {
    node_id: NodeId,
    offset: TextOffset,
    affinity: CursorAffinity,
}

impl TextPoint {
    /// Creates an unchecked point.
    ///
    /// Construction does not touch a document because coordinates may be
    /// created before or after the snapshots they refer to. Every use against
    /// a snapshot goes through [`TextPoint::validate`].
    #[must_use]
    pub const fn new(node_id: NodeId, offset: TextOffset, affinity: CursorAffinity) -> Self {
        Self {
            node_id,
            offset,
            affinity,
        }
    }

    /// Creates an unchecked point with the default affinity.
    #[must_use]
    pub const fn at_start_of(node_id: NodeId) -> Self {
        Self::new(node_id, TextOffset::ZERO, CursorAffinity::Before)
    }

    /// Returns the target node identity.
    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    /// Returns the text coordinate inside the node's inline content.
    #[must_use]
    pub const fn offset(self) -> TextOffset {
        self.offset
    }

    /// Returns the visual affinity.
    #[must_use]
    pub const fn affinity(self) -> CursorAffinity {
        self.affinity
    }

    /// Returns the point with `affinity`, leaving identity and offset intact.
    #[must_use]
    pub const fn with_affinity(self, affinity: CursorAffinity) -> Self {
        Self { affinity, ..self }
    }

    /// Validates this point against one document snapshot.
    pub fn validate(&self, document: &XiaomuDocument) -> Result<()> {
        let Some(node) = document.node(self.node_id) else {
            return Err(Error::UnknownNode);
        };

        let Some(inline) = node.content().as_inline() else {
            return Err(Error::InvalidSelection);
        };

        inline.validate_offset(self.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    };

    fn paragraph_document(text: &str) -> (XiaomuDocument, NodeId) {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        (
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            paragraph,
        )
    }

    #[test]
    fn validate_rejects_unknown_nodes() {
        let (document, paragraph) = paragraph_document("abc");
        let missing = NodeId::from_allocated(999);

        assert_eq!(
            TextPoint::at_start_of(missing).validate(&document),
            Err(Error::UnknownNode)
        );
        let _ = paragraph;
    }

    #[test]
    fn validate_rejects_non_inline_targets() {
        let mut builder = NodeStoreBuilder::new();
        let rule = builder
            .insert(
                NodeKind::HorizontalRule,
                NodeAttrs::empty(),
                NodeContent::Atomic,
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([rule]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();

        assert_eq!(
            TextPoint::at_start_of(rule).validate(&document),
            Err(Error::InvalidSelection)
        );
    }

    #[test]
    fn empty_inline_content_accepts_only_offset_zero() {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();

        assert_eq!(
            TextPoint::at_start_of(paragraph).validate(&document),
            Ok(())
        );
        let past_end = TextOffset::from_validated_byte_index(1);
        assert_eq!(
            TextPoint::new(paragraph, past_end, CursorAffinity::Before).validate(&document),
            Err(Error::TextOutOfBounds { offset: 1, len: 0 })
        );
    }
}
