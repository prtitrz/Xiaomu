//! P4 mixed-inline position projection from the current document view.
//!
//! P4.1 keeps the P0-P3 rendering/navigation engine unchanged while exposing
//! a frontend API in the coordinate vocabulary that canonical atoms will use.
//! Once P4.2 upgrades Runtime storage, callers keep using this projection seam.

use xiaomu_core::selection::InlinePoint;

use crate::document_view::DocumentView;

impl DocumentView {
    /// Projects the current selection focus into the mixed-inline coordinate.
    ///
    /// Structural gap selections have no inline position and return `None`.
    /// On P4.1 text-only documents this always carries atom ordinal zero.
    #[must_use]
    pub fn inline_focus_point(&self) -> Option<InlinePoint> {
        self.session()
            .borrow()
            .selection()
            .focus()
            .as_inline_point()
    }

    /// Projects both selection endpoints into mixed-inline coordinates.
    ///
    /// Returns `None` when either endpoint is structural. Keeping that case
    /// explicit prevents GPUI code from inventing an inline coordinate for a
    /// document gap.
    #[must_use]
    pub fn inline_selection_points(&self) -> Option<(InlinePoint, InlinePoint)> {
        let session = self.session().borrow();
        let selection = session.selection();
        Some((
            selection.anchor().as_inline_point()?,
            selection.focus().as_inline_point()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use xiaomu_core::document::{
        InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
        XiaomuDocument,
    };
    use xiaomu_core::selection::{CursorAffinity, NodeGap, TextPoint};
    use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

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
    fn document_view_projects_text_selection_into_inline_points() {
        let (document, paragraph) = fixture();
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(4)
            .unwrap();
        let point = TextPoint::new(paragraph, offset, CursorAffinity::After);
        let session = DocumentSession::new(document, DocumentSelection::collapsed(point)).unwrap();
        let view = DocumentView::new(Rc::new(RefCell::new(session)));

        let projected = InlinePoint::from(point);
        assert_eq!(view.inline_focus_point(), Some(projected));
        assert_eq!(view.inline_selection_points(), Some((projected, projected)));
    }

    #[test]
    fn structural_gap_has_no_fake_inline_projection() {
        let (document, _) = fixture();
        let gap = NodeGap::new(document.root(), 0);
        let session = DocumentSession::new(document, DocumentSelection::collapsed(gap)).unwrap();
        let view = DocumentView::new(Rc::new(RefCell::new(session)));

        assert_eq!(view.inline_focus_point(), None);
        assert_eq!(view.inline_selection_points(), None);
    }
}
