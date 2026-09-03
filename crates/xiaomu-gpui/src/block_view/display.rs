//! Visual projection for one mixed-inline paragraph.
//!
//! Platform text input intentionally keeps using the canonical editable-text
//! projection from [`project_display_content`]. Layout and paint use the
//! atom-aware projection in this module so renderer bytes never leak back into
//! Core coordinates.

use xiaomu_core::document::{InlineContent, MarkKind, NodeId};
use xiaomu_core::selection::InlinePoint;
use xiaomu_runtime::session::DocumentPosition;

use crate::inline_atom_display::InlineAtomDisplayProjection;

use super::{DisplaySegment, ParagraphView, SelectionProjection};

pub(super) fn project_display_content(
    inline: &InlineContent,
    composition: Option<(std::ops::Range<usize>, &str)>,
) -> (String, Vec<DisplaySegment>) {
    let (base_start, base_end, preedit) = composition
        .as_ref()
        .map(|(range, text)| (range.start, range.end, *text))
        .unwrap_or((usize::MAX, usize::MAX, ""));
    let replaced_len = base_end.saturating_sub(base_start);

    let mut segments = Vec::new();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;

        let style = style_for_run(run.marks());
        let mut push_piece = |start: usize, end: usize, display_start: usize| {
            if start < end {
                segments.push(DisplaySegment {
                    start: display_start,
                    text: run.text().as_str()[start - run_start..end - run_start].to_owned(),
                    bold: style.0,
                    italic: style.1,
                    underline: style.2,
                    strike: style.3,
                    code: style.4,
                });
            }
        };

        let prefix_end = run_end.min(base_start);
        push_piece(run_start, prefix_end, run_start);

        let suffix_start = run_start.max(base_end);
        let suffix_display_start = suffix_start.saturating_sub(replaced_len) + preedit.len();
        push_piece(suffix_start, run_end, suffix_display_start);
    }

    if let Some((range, text)) = composition {
        segments.push(DisplaySegment {
            start: range.start,
            text: text.to_owned(),
            bold: false,
            italic: false,
            underline: true,
            strike: false,
            code: false,
        });
    }

    normalize_segments(segments)
}

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
            text: run.text().as_str()[overlap_start - run_start..overlap_end - run_start].to_owned(),
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
    segments.sort_by_key(|segment| segment.start);
    let mut text = String::new();
    for segment in &mut segments {
        segment.start = text.len();
        text.push_str(&segment.text);
    }
    (text, segments)
}

impl ParagraphView {
    /// Builds the canonical editable-text projection used by platform input.
    #[must_use]
    pub(crate) fn display_content(&self) -> (String, Vec<DisplaySegment>) {
        let Some(inline) = self.inline() else {
            return (String::new(), Vec::new());
        };
        let composition = self
            .composition
            .as_ref()
            .map(|state| (state.base_range(), state.preedit()));
        project_display_content(&inline, composition)
    }

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
            return project_display_content(&inline, None);
        };
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
    /// their full [`InlinePoint`] ordinal, so a selection spanning one atom at a
    /// zero-byte canonical seam still paints the renderer span.
    #[must_use]
    pub(crate) fn projected_selection(&self, order: &[NodeId]) -> SelectionProjection {
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

        let Some(projection) = self.atom_display_projection() else {
            return SelectionProjection::None;
        };
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

    pub(super) fn display_offset_for_focus(&self, point: InlinePoint) -> Option<usize> {
        self.atom_display_projection()?
            .display_offset_for_inline_point(point)
    }
}
