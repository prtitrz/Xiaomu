//! Visual projection for one mixed-inline paragraph.
//!
//! Platform text input intentionally keeps using the canonical editable-text
//! projection owned by [`super::ParagraphView::display_content`]. Layout and
//! paint use the atom-aware projection in this module so renderer bytes never
//! leak back into Core coordinates.

use xiaomu_core::document::{InlineContent, MarkKind, NodeId};
use xiaomu_core::selection::CursorAffinity;
use xiaomu_runtime::session::DocumentPosition;

use crate::inline_atom_display::InlineAtomDisplayProjection;

use super::{DisplaySegment, ParagraphView, SelectionProjection};

fn project_atom_display_content(
    inline: &InlineContent,
    projection: &InlineAtomDisplayProjection,
) -> (String, Vec<DisplaySegment>) {
    let mut segments = Vec::new();
    let mut canonical_cursor = 0usize;

    for atom in projection.atoms() {
        let anchor = atom.text_offset().as_usize();
        push_styled_text(inline, canonical_cursor, anchor, &mut segments);
        let display_range = atom.display_range().clone();
        let rendered = &projection.display_text()[display_range];
        segments.push(DisplaySegment {
            start: 0,
            text: rendered.to_owned(),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            code: false,
        });
        canonical_cursor = anchor;
    }

    push_styled_text(
        inline,
        canonical_cursor,
        projection.canonical_text().len(),
        &mut segments,
    );
    let result = normalize_segments(segments);
    debug_assert_eq!(result.0, projection.display_text());
    result
}

fn push_styled_text(
    inline: &InlineContent,
    start: usize,
    end: usize,
    segments: &mut Vec<DisplaySegment>,
) {
    if start >= end {
        return;
    }

    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;

        let overlap_start = start.max(run_start);
        let overlap_end = end.min(run_end);
        if overlap_start >= overlap_end {
            continue;
        }

        let style = style_for_run(run.marks());
        segments.push(DisplaySegment {
            start: 0,
            text: run.text().as_str()[overlap_start - run_start..overlap_end - run_start]
                .to_owned(),
            bold: style.0,
            italic: style.1,
            underline: style.2,
            strike: style.3,
            code: style.4,
        });
    }
}

fn style_for_run(marks: &xiaomu_core::document::MarkSet) -> (bool, bool, bool, bool, bool) {
    (
        marks.contains(MarkKind::Bold),
        marks.contains(MarkKind::Italic),
        marks.contains(MarkKind::Underline),
        marks.contains(MarkKind::Strike),
        marks.contains(MarkKind::Code),
    )
}

fn normalize_segments(mut segments: Vec<DisplaySegment>) -> (String, Vec<DisplaySegment>) {
    let mut text = String::new();
    for segment in &mut segments {
        segment.start = text.len();
        text.push_str(&segment.text);
    }
    (text, segments)
}

impl ParagraphView {
    /// Builds the visual layout projection.
    ///
    /// During IME composition the existing editable projection remains the
    /// source of truth. Outside composition every canonical atom is spliced via
    /// the current renderer registry and receives a real display span.
    #[must_use]
    pub(crate) fn layout_content(&self) -> (String, Vec<DisplaySegment>) {
        if self.is_composing() {
            return self.display_content();
        }
        let Some(inline) = self.inline() else {
            return (String::new(), Vec::new());
        };
        let Some(projection) = self.atom_display_projection() else {
            return self.display_content();
        };
        if projection.atoms().is_empty() {
            return self.display_content();
        }
        project_atom_display_content(&inline, &projection)
    }

    /// Returns the current canonical-to-visual atom projection for this block.
    #[must_use]
    pub(crate) fn atom_display_projection(&self) -> Option<InlineAtomDisplayProjection> {
        let session = self.session.borrow();
        InlineAtomDisplayProjection::build(session.document(), self.node, &self.atom_renderers)
    }

    /// Projects the document selection onto this block's visual display bytes.
    ///
    /// `order` lists inline-bearing nodes in document order. Endpoints retain
    /// their full mixed-inline ordinal, so a selection spanning one atom at a
    /// zero-byte canonical seam still paints the renderer span.
    #[must_use]
    pub(crate) fn projected_display_selection(&self, order: &[NodeId]) -> SelectionProjection {
        let projection = match self.atom_display_projection() {
            Some(projection) if !projection.atoms().is_empty() => projection,
            Some(_) => return self.projected_selection(order),
            None => return SelectionProjection::None,
        };

        let session = self.session.borrow();
        let selection = session.selection();
        let document = session.document();
        let endpoint = |position: DocumentPosition| match position {
            DocumentPosition::Inline(point) => Some(point),
            DocumentPosition::Gap(_) => None,
        };
        let Ok((head, tail)) = selection.ordered(document) else {
            return SelectionProjection::None;
        };
        let Some(head) = endpoint(head) else {
            return SelectionProjection::None;
        };
        let Some(tail) = endpoint(tail) else {
            return SelectionProjection::None;
        };
        let Some(my_index) = order.iter().position(|id| *id == self.node) else {
            return SelectionProjection::None;
        };
        let Some(head_index) = order.iter().position(|id| *id == head.node_id()) else {
            return SelectionProjection::None;
        };
        let Some(tail_index) = order.iter().position(|id| *id == tail.node_id()) else {
            return SelectionProjection::None;
        };
        if my_index < head_index || my_index > tail_index {
            return SelectionProjection::None;
        }

        let display_len = projection.display_text().len();
        let start = if head.node_id() == self.node {
            let Some(start) = projection.display_offset_for_inline_point(head) else {
                return SelectionProjection::None;
            };
            start
        } else {
            0
        };
        let end = if tail.node_id() == self.node {
            let Some(end) = projection.display_offset_for_inline_point(tail) else {
                return SelectionProjection::None;
            };
            end
        } else {
            display_len
        };

        if start >= end {
            SelectionProjection::Caret(start.min(display_len))
        } else {
            SelectionProjection::Highlight {
                start,
                end: end.min(display_len),
            }
        }
    }

    /// Projects the focused mixed-inline caret into the visual byte space.
    #[must_use]
    pub(crate) fn display_focus_caret(&self) -> Option<(usize, CursorAffinity)> {
        let projection = self.atom_display_projection()?;
        if projection.atoms().is_empty() {
            return self.focus_caret();
        }
        let point = match self.session.borrow().selection().focus() {
            DocumentPosition::Inline(point) if point.node_id() == self.node => point,
            _ => return None,
        };
        let display = projection.display_offset_for_inline_point(point)?;
        Some((display, point.affinity()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline_atom::InlineAtomRendererRegistry;
    use xiaomu_core::document::{
        AtomKind, InlineAtomContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    };
    use xiaomu_core::selection::{CursorAffinity, InlinePoint};
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

    #[test]
    fn atom_display_segment_preserves_neighbor_text_marks() {
        let plain = MarkSet::empty();
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        let inline = InlineContent::new([
            TextRun::new("A", plain).unwrap(),
            TextRun::new("中B", bold).unwrap(),
        ])
        .unwrap();

        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline),
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
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(1)
            .unwrap();
        let document = Transaction::new(TransactionOrigin::Extension(
            "layout-projection-test".into(),
        ))
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(paragraph, offset, 0, CursorAffinity::Before),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("@Ann").unwrap(),
        })
        .apply(&document)
        .unwrap();

        let renderers = InlineAtomRendererRegistry::new();
        let projection =
            InlineAtomDisplayProjection::build(&document, paragraph, &renderers).unwrap();
        let inline = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        let (display, segments) = project_atom_display_content(inline, &projection);

        assert_eq!(display, "A@Ann中B");
        assert_eq!(segments.len(), 3);
        assert_eq!((segments[0].start, segments[0].text.as_str()), (0, "A"));
        assert!(!segments[0].bold);
        assert_eq!((segments[1].start, segments[1].text.as_str()), (1, "@Ann"));
        assert!(!segments[1].bold);
        assert!(!segments[1].italic);
        assert!(!segments[1].underline);
        assert!(!segments[1].strike);
        assert!(!segments[1].code);
        assert_eq!((segments[2].start, segments[2].text.as_str()), (5, "中B"));
        assert!(segments[2].bold);
    }
}
