//! Mixed-inline caret positions that preserve the UTF-8 text coordinate contract.

use crate::document::{NodeId, XiaomuDocument};
use crate::text::TextOffset;
use crate::{Error, Result};

use super::{CursorAffinity, TextPoint};

/// A stable caret position inside one inline-bearing document node.
///
/// `text_offset` remains a validated UTF-8 byte offset into the node's
/// concatenated canonical text. `atom_index` is orthogonal to that text
/// coordinate: when multiple inline atoms are anchored at the same text
/// boundary, it counts how many of those atoms are before the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InlinePoint {
    node_id: NodeId,
    text_offset: TextOffset,
    atom_index: usize,
    affinity: CursorAffinity,
}

impl InlinePoint {
    /// Creates an unchecked mixed-inline point.
    ///
    /// Every use against a snapshot must call [`InlinePoint::validate`].
    #[must_use]
    pub const fn new(
        node_id: NodeId,
        text_offset: TextOffset,
        atom_index: usize,
        affinity: CursorAffinity,
    ) -> Self {
        Self {
            node_id,
            text_offset,
            atom_index,
            affinity,
        }
    }

    /// Creates the start position of an inline-bearing node.
    #[must_use]
    pub const fn at_start_of(node_id: NodeId) -> Self {
        Self::new(node_id, TextOffset::ZERO, 0, CursorAffinity::Before)
    }

    /// Returns the target inline-bearing node identity.
    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    /// Returns the UTF-8 text coordinate of this point.
    #[must_use]
    pub const fn text_offset(self) -> TextOffset {
        self.text_offset
    }

    /// Returns how many same-boundary atoms are before this caret.
    #[must_use]
    pub const fn atom_index(self) -> usize {
        self.atom_index
    }

    /// Returns the visual affinity.
    #[must_use]
    pub const fn affinity(self) -> CursorAffinity {
        self.affinity
    }

    /// Returns the same canonical position with a different visual affinity.
    #[must_use]
    pub const fn with_affinity(self, affinity: CursorAffinity) -> Self {
        Self { affinity, ..self }
    }

    /// Returns the text-only compatibility point when no atom ordinal is used.
    ///
    /// A non-zero atom ordinal cannot be represented by [`TextPoint`], so this
    /// conversion fails closed instead of silently discarding information.
    pub fn to_text_point(self) -> Result<TextPoint> {
        if self.atom_index != 0 {
            return Err(Error::InvalidSelection);
        }
        Ok(TextPoint::new(
            self.node_id,
            self.text_offset,
            self.affinity,
        ))
    }

    /// Validates this point against one document snapshot.
    ///
    /// If `N` atoms are anchored at `text_offset`, `atom_index` must be in
    /// `0..=N`. The text component is validated independently as a UTF-8 byte
    /// boundary, so atoms never consume fake bytes.
    pub fn validate(&self, document: &XiaomuDocument) -> Result<()> {
        let Some(node) = document.node(self.node_id) else {
            return Err(Error::UnknownNode);
        };
        let Some(inline) = node.content().as_inline() else {
            return Err(Error::InvalidSelection);
        };
        inline.validate_offset(self.text_offset)?;
        if self.atom_index > inline.atom_count_at(self.text_offset) {
            return Err(Error::InvalidSelection);
        }
        Ok(())
    }
}

impl From<TextPoint> for InlinePoint {
    fn from(point: TextPoint) -> Self {
        Self::new(point.node_id(), point.offset(), 0, point.affinity())
    }
}

impl TryFrom<InlinePoint> for TextPoint {
    type Error = Error;

    fn try_from(point: InlinePoint) -> Result<Self> {
        point.to_text_point()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, MarkSet, NodeAttrs,
        NodeContent, NodeKind, NodeStoreBuilder, TextRun,
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

    fn adjacent_atom_document() -> (XiaomuDocument, NodeId, TextOffset) {
        let mut builder = NodeStoreBuilder::new();
        let first = builder
            .insert(
                NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::new("@A").unwrap()),
            )
            .unwrap();
        let second = builder
            .insert(
                NodeKind::InlineAtom(AtomKind::new("reference").unwrap()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::new("ref").unwrap()),
            )
            .unwrap();
        let at = TextOffset::from_validated_byte_index(1);
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::with_atoms(
                        [TextRun::new("ab", MarkSet::empty()).unwrap()],
                        [
                            InlineAtomPlacement::new(first, at),
                            InlineAtomPlacement::new(second, at),
                        ],
                    )
                    .unwrap(),
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
            at,
        )
    }

    #[test]
    fn text_point_conversion_preserves_existing_coordinates_exactly() {
        let (document, paragraph) = paragraph_document("a中👍");
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(4)
            .unwrap();
        let text = TextPoint::new(paragraph, offset, CursorAffinity::After);

        let inline = InlinePoint::from(text);
        assert_eq!(inline.node_id(), paragraph);
        assert_eq!(inline.text_offset(), offset);
        assert_eq!(inline.atom_index(), 0);
        assert_eq!(inline.affinity(), CursorAffinity::After);
        assert_eq!(inline.validate(&document), Ok(()));
        assert_eq!(inline.to_text_point(), Ok(text));
    }

    #[test]
    fn adjacent_atoms_expose_three_unique_caret_gaps() {
        let (document, paragraph, offset) = adjacent_atom_document();

        for atom_index in 0..=2 {
            assert_eq!(
                InlinePoint::new(paragraph, offset, atom_index, CursorAffinity::Before)
                    .validate(&document),
                Ok(())
            );
        }
        assert_eq!(
            InlinePoint::new(paragraph, offset, 3, CursorAffinity::Before).validate(&document),
            Err(Error::InvalidSelection)
        );
        assert_eq!(
            InlinePoint::new(paragraph, offset, 1, CursorAffinity::Before).to_text_point(),
            Err(Error::InvalidSelection)
        );
    }

    #[test]
    fn nonzero_atom_index_is_rejected_at_plain_text_boundary() {
        let (document, paragraph) = paragraph_document("abc");
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(1)
            .unwrap();
        let inline = InlinePoint::new(paragraph, offset, 1, CursorAffinity::Before);

        assert_eq!(inline.validate(&document), Err(Error::InvalidSelection));
        assert_eq!(inline.to_text_point(), Err(Error::InvalidSelection));
    }

    #[test]
    fn validation_keeps_utf8_boundary_checks_in_text_layer() {
        let (document, paragraph) = paragraph_document("a中");
        let invalid = TextOffset::from_validated_byte_index(2);
        let inline = InlinePoint::new(paragraph, invalid, 0, CursorAffinity::Before);

        assert_eq!(
            inline.validate(&document),
            Err(Error::InvalidTextBoundary { offset: 2 })
        );
    }
}
