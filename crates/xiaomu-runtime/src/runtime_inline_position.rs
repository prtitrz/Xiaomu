//! P4.1 Runtime compatibility for Core mixed-inline positions.
//!
//! Runtime still stores the proven P0-P3 `TextPoint | NodeGap` position model
//! until canonical atom placements exist in P4.2. This seam accepts and
//! projects [`InlinePoint`] without discarding atom ordinal information:
//! ordinal zero converts exactly, while a non-zero ordinal fails closed.

use xiaomu_core::selection::InlinePoint;

use crate::session::{DocumentPosition, DocumentSelection, SessionError};

impl DocumentPosition {
    /// Adapts one Core mixed-inline point into the current Runtime position.
    ///
    /// P4.1 documents contain no canonical inline atoms, so only
    /// `atom_index == 0` is representable. Non-zero ordinals fail closed and
    /// become representable only when P4.2 upgrades Runtime storage together
    /// with canonical atom placement validation.
    pub fn from_inline_point(point: InlinePoint) -> Result<Self, SessionError> {
        point
            .to_text_point()
            .map(Self::Text)
            .map_err(|_| SessionError::SelectionInvalid)
    }

    /// Projects this Runtime endpoint into the mixed-inline coordinate seam.
    ///
    /// Structural gaps have no inline coordinate and return `None`. Existing
    /// text endpoints map exactly to an [`InlinePoint`] with atom ordinal zero.
    #[must_use]
    pub fn as_inline_point(self) -> Option<InlinePoint> {
        match self {
            Self::Text(point) => Some(InlinePoint::from(point)),
            Self::Gap(_) => None,
        }
    }
}

impl TryFrom<InlinePoint> for DocumentPosition {
    type Error = SessionError;

    fn try_from(point: InlinePoint) -> Result<Self, Self::Error> {
        Self::from_inline_point(point)
    }
}

impl DocumentSelection {
    /// Creates a text-like document selection from two mixed-inline endpoints.
    ///
    /// This is the P4.1 migration entry point for callers that already speak
    /// [`InlinePoint`]. It preserves the existing document-level selection
    /// semantics and rejects atom ordinals that the current canonical model
    /// cannot yet represent.
    pub fn from_inline_points(
        anchor: InlinePoint,
        focus: InlinePoint,
    ) -> Result<Self, SessionError> {
        Ok(Self::new(
            DocumentPosition::from_inline_point(anchor)?,
            DocumentPosition::from_inline_point(focus)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{
        InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
        XiaomuDocument,
    };
    use xiaomu_core::selection::{CursorAffinity, NodeGap, TextPoint};

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

        let runtime = DocumentPosition::from_inline_point(mixed).unwrap();
        assert_eq!(runtime, DocumentPosition::Text(text));
        assert_eq!(runtime.as_inline_point(), Some(mixed));

        let selection = DocumentSelection::from_inline_points(mixed, mixed).unwrap();
        assert!(selection.is_collapsed());
        assert_eq!(selection.validate(&document), Ok(()));
    }

    #[test]
    fn nonzero_atom_ordinal_is_never_silently_dropped() {
        let (document, paragraph) = fixture();
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(1)
            .unwrap();
        let mixed = InlinePoint::new(paragraph, offset, 1, CursorAffinity::Before);

        assert_eq!(
            DocumentPosition::from_inline_point(mixed),
            Err(SessionError::SelectionInvalid)
        );
        assert_eq!(
            DocumentSelection::from_inline_points(mixed, mixed),
            Err(SessionError::SelectionInvalid)
        );
    }

    #[test]
    fn structural_gap_does_not_pretend_to_be_inline() {
        let (document, _) = fixture();
        let gap = DocumentPosition::Gap(NodeGap::new(document.root(), 0));
        assert_eq!(gap.as_inline_point(), None);
    }
}
