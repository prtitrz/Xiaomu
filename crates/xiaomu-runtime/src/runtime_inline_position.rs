//! Runtime support for Core mixed-inline positions.
//!
//! Since P4.3 the Runtime stores the full mixed-inline coordinate
//! `(text_offset, atom_index)`: a caret between same-boundary atoms is a
//! first-class [`DocumentPosition::Inline`] endpoint. Plain-text positions
//! keep `atom_index == 0` and stay exactly equivalent to the P0-P3
//! `TextPoint` coordinate space. Ordinal validity is enforced by
//! [`DocumentSelection::validate`] against the current snapshot, mirroring
//! how every other stale coordinate is checked before use.

use xiaomu_core::selection::InlinePoint;

use crate::session::{DocumentPosition, DocumentSelection};

impl DocumentPosition {
    /// Adapts one Core mixed-inline point into the Runtime position.
    ///
    /// The atom ordinal is preserved. Validation against a snapshot (node
    /// exists, UTF-8 boundary, ordinal within `0..=N`) happens through
    /// [`DocumentSelection::validate`], exactly like for any other stale
    /// coordinate the Runtime is handed.
    #[must_use]
    pub fn from_inline_point(point: InlinePoint) -> Self {
        Self::Inline(point)
    }

    /// Projects this Runtime endpoint into the mixed-inline coordinate seam.
    ///
    /// Structural gaps have no inline coordinate and return `None`. Inline
    /// endpoints project exactly, including a non-zero atom ordinal.
    #[must_use]
    pub fn as_inline_point(self) -> Option<InlinePoint> {
        match self {
            Self::Inline(point) => Some(point),
            Self::Gap(_) => None,
        }
    }
}

impl From<InlinePoint> for DocumentPosition {
    fn from(point: InlinePoint) -> Self {
        Self::Inline(point)
    }
}

impl DocumentSelection {
    /// Creates a document selection from two mixed-inline endpoints.
    ///
    /// This is the mixed-inline entry point for callers that already speak
    /// [`InlinePoint`]. It preserves the document-level selection semantics;
    /// both endpoints must later validate against the current snapshot.
    pub fn from_inline_points(anchor: InlinePoint, focus: InlinePoint) -> Self {
        Self::new(
            DocumentPosition::from_inline_point(anchor),
            DocumentPosition::from_inline_point(focus),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionError;
    use xiaomu_core::document::{
        AtomKind, InlineAtomContent, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    };
    use xiaomu_core::selection::{CursorAffinity, NodeGap, TextPoint};
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

    fn fixture() -> (XiaomuDocument, xiaomu_core::document::NodeId) {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("a中👍", MarkSet::empty()).unwrap()]).unwrap(),
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

    fn with_atom(
        document: &XiaomuDocument,
        paragraph: xiaomu_core::document::NodeId,
    ) -> XiaomuDocument {
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(1)
            .unwrap();
        Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertInlineAtom {
                at: InlinePoint::new(paragraph, offset, 0, CursorAffinity::Before),
                kind: AtomKind::new("mention").unwrap(),
                attrs: NodeAttrs::empty(),
                content: InlineAtomContent::new("@A").unwrap(),
            })
            .apply(document)
            .unwrap()
    }

    #[test]
    fn existing_text_position_round_trips_through_runtime_seam() {
        let (document, paragraph) = fixture();
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(4)
            .unwrap();
        let text = TextPoint::new(paragraph, offset, CursorAffinity::After);
        let mixed = InlinePoint::from(text);

        let runtime = DocumentPosition::from_inline_point(mixed);
        assert_eq!(runtime, DocumentPosition::Inline(mixed));
        assert_eq!(runtime.as_inline_point(), Some(mixed));

        let selection = DocumentSelection::from_inline_points(mixed, mixed);
        assert!(selection.is_collapsed());
        assert_eq!(selection.validate(&document), Ok(()));
    }

    #[test]
    fn nonzero_atom_ordinal_stays_representable_and_validates() {
        let (document, paragraph) = fixture();
        let with_atom = with_atom(&document, paragraph);
        let offset = with_atom
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(1)
            .unwrap();
        let seam = InlinePoint::new(paragraph, offset, 1, CursorAffinity::Before);

        let selection = DocumentSelection::collapsed(seam);
        assert_eq!(selection.validate(&with_atom), Ok(()));
        assert_eq!(
            selection.validate(&document),
            Err(SessionError::SelectionInvalid)
        );

        // Ordinal 0 remains the plain-text gap before the atom.
        let before = DocumentSelection::collapsed(InlinePoint::new(
            paragraph,
            offset,
            0,
            CursorAffinity::Before,
        ));
        assert_eq!(before.validate(&with_atom), Ok(()));
    }

    #[test]
    fn structural_gap_does_not_pretend_to_be_inline() {
        let (document, _) = fixture();
        let gap = DocumentPosition::Gap(NodeGap::new(document.root(), 0));
        assert_eq!(gap.as_inline_point(), None);
    }
}
