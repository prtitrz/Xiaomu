//! Wrapped text geometry for one inline-bearing block.
//!
//! This layer is deliberately GPUI-local. Canonical positions remain Core
//! [`xiaomu_core::selection::TextPoint`] values; soft-wrap only projects byte
//! offsets into visual rows and pixels.

use std::ops::Range;

use crate::block_alignment::BlockAlignment;
#[cfg(test)]
use gpui::{Bounds, px};
use gpui::{Pixels, Point, Size, WrappedLine, point, size};
use xiaomu_core::selection::CursorAffinity;

#[path = "aligned_decorations.rs"]
mod aligned_decorations;
#[path = "mixed_layout.rs"]
mod mixed_layout;
#[path = "selection_layout.rs"]
mod selection_layout;

#[cfg(test)]
#[path = "alignment_layout_tests.rs"]
mod alignment_tests;

type SelectionPositionIndex = std::rc::Rc<std::cell::OnceCell<Vec<Vec<(usize, Pixels)>>>>;

/// Measured wrapped text for one block.
///
/// `WrappedLine` here means one logical line as understood by GPUI; each one
/// may itself contain several soft-wrapped visual rows. Paragraphs currently
/// contain one logical line, while keeping this representation multi-line
/// ready avoids rebuilding the geometry layer when CodeBlock gains newline
/// semantics later in P3.
#[derive(Clone, Debug)]
pub(crate) struct BlockTextLayout {
    lines: Vec<WrappedLine>,
    selection_positions: SelectionPositionIndex,
    mixed: Option<std::rc::Rc<crate::mixed_size::MixedLayout>>,
    line_height: Pixels,
    sized_font_size: Option<Pixels>,
    unavailable: bool,
    size: Size<Pixels>,
    rows: Vec<VisualRow>,
    alignment: BlockAlignment,
    alignment_enabled: bool,
    alignment_width: Pixels,
    paint_lines: Option<Vec<WrappedLine>>,
    decoration_runs: Vec<gpui::TextRun>,
    decorations: Vec<aligned_decorations::Stroke>,
}

impl BlockTextLayout {
    pub(super) fn new(lines: Vec<WrappedLine>, line_height: Pixels) -> Self {
        let mut measured = size(Pixels::ZERO, Pixels::ZERO);
        for line in &lines {
            let line_size = line.size(line_height);
            measured.width = measured.width.max(line_size.width).ceil();
            measured.height += line_size.height;
        }
        measured.height = measured.height.max(line_height);
        let mut layout = Self {
            lines,
            selection_positions: Default::default(),
            mixed: None,
            line_height,
            sized_font_size: None,
            unavailable: false,
            size: measured,
            rows: Vec::new(),
            alignment: BlockAlignment::Left,
            alignment_enabled: false,
            alignment_width: measured.width,
            paint_lines: None,
            decoration_runs: Vec::new(),
            decorations: Vec::new(),
        };
        layout.rows = layout.measure_rows();
        layout
    }

    /// Painting and all coordinate consumers use this exact text-box width.
    pub(super) fn aligned(mut self, alignment: BlockAlignment, width: Pixels) -> Self {
        self.alignment = alignment;
        self.alignment_enabled = true;
        self.alignment_width = width;
        for row in &mut self.rows {
            row.x = alignment.offset(width, row.width);
        }
        self
    }

    pub(super) fn with_alignment(self, alignment: Option<BlockAlignment>, width: Pixels) -> Self {
        match alignment {
            Some(alignment) => self.aligned(alignment, width),
            None => self,
        }
    }

    pub(super) fn has_alignment(&self) -> bool {
        self.alignment_enabled
    }

    pub(super) fn alignment(&self) -> BlockAlignment {
        self.alignment
    }

    pub(super) fn alignment_width(&self) -> Pixels {
        self.alignment_width
    }

    pub(super) fn size(&self) -> Size<Pixels> {
        self.size
    }

    pub(super) fn line_height(&self) -> Pixels {
        self.line_height
    }

    #[cfg(test)]
    pub(super) fn lines(&self) -> &[WrappedLine] {
        &self.lines
    }

    pub(super) fn paint_lines(&self) -> &[WrappedLine] {
        self.paint_lines.as_deref().unwrap_or(&self.lines)
    }

    pub(super) fn with_decoration_carrier(
        mut self,
        mut carrier: Vec<WrappedLine>,
        runs: Vec<gpui::TextRun>,
    ) -> Self {
        assert_eq!(carrier.len(), self.lines.len());
        for (paint, original) in carrier.iter_mut().zip(&self.lines) {
            // Only decoration metadata comes from the stripped shape. Keep
            // the original glyph/cluster/wrap layout byte-for-byte identical.
            assert_eq!(paint.text, original.text);
            **paint = std::sync::Arc::clone(&**original);
        }
        self.paint_lines = Some(carrier);
        self.decoration_runs = runs;
        self.decorations = self.decoration_strokes();
        self
    }

    pub(super) fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        if self.unavailable {
            return None;
        }
        if self.mixed.is_some() {
            return self.mixed_position(index, CursorAffinity::Before);
        }
        let row_ix = row_for_caret(&self.rows, index, CursorAffinity::Before)?;
        let row = &self.rows[row_ix];
        let offset = row.x;
        if self.alignment_enabled
            && let Some(line) = self.lines.get(row.logical_line)
        {
            return Some(point(
                row.x + line.unwrapped_layout.x_for_index(index - row.logical_start) - row.start_x,
                row.y,
            ));
        }
        let mut logical_start = 0usize;
        let mut y = Pixels::ZERO;

        for line in &self.lines {
            let logical_end = logical_start + line.len();
            if index <= logical_end {
                let local = index - logical_start;
                return line
                    .position_for_index(local, self.line_height)
                    .map(|position| point(position.x + offset, position.y + y));
            }
            logical_start = logical_end.saturating_add(1);
            y += line.size(self.line_height).height;
        }

        if self.lines.is_empty() && index == 0 {
            Some(point(offset, Pixels::ZERO))
        } else {
            None
        }
    }

    pub(crate) fn position_for_caret(
        &self,
        index: usize,
        affinity: CursorAffinity,
    ) -> Option<Point<Pixels>> {
        if self.mixed.is_some() {
            return self.mixed_position(index, affinity);
        }
        let rows = self.visual_rows();
        let row_ix = row_for_caret(rows, index, affinity)?;
        let row = &rows[row_ix];

        if affinity.is_after()
            && row_ix > 0
            && row.range.start == index
            && rows[row_ix - 1].range.end == index
        {
            return Some(point(row.x, row.y));
        }

        self.position_for_index(index)
    }

    pub(crate) fn caret_x(&self, index: usize, affinity: CursorAffinity) -> Option<Pixels> {
        self.position_for_caret(index, affinity)
            .map(|position| position.x)
    }

    pub(crate) fn is_soft_wrap_boundary(&self, index: usize) -> bool {
        self.visual_rows()
            .windows(2)
            .any(|rows| rows[0].range.end == index && rows[1].range.start == index)
    }

    pub(crate) fn vertical_target(
        &self,
        index: usize,
        affinity: CursorAffinity,
        desired_x: Pixels,
        down: bool,
    ) -> Option<(usize, CursorAffinity)> {
        let rows = self.visual_rows();
        let current = row_for_caret(rows, index, affinity)?;
        let target = if down {
            current
                .checked_add(1)
                .filter(|target| *target < rows.len())?
        } else {
            current.checked_sub(1)?
        };
        Some(self.target_for_row_x(rows, target, desired_x))
    }

    pub(crate) fn edge_row_target(
        &self,
        desired_x: Pixels,
        last: bool,
    ) -> Option<(usize, CursorAffinity)> {
        let rows = self.visual_rows();
        let row_ix = if last { rows.len().checked_sub(1)? } else { 0 };
        Some(self.target_for_row_x(rows, row_ix, desired_x))
    }

    pub(crate) fn visual_line_edge(
        &self,
        index: usize,
        affinity: CursorAffinity,
        to_end: bool,
    ) -> Option<(usize, CursorAffinity)> {
        let rows = self.visual_rows();
        let row_ix = row_for_caret(rows, index, affinity)?;
        let row = &rows[row_ix];
        if to_end {
            Some((row.range.end, CursorAffinity::Before))
        } else {
            Some((row.range.start, affinity_for_row_start(rows, row_ix)))
        }
    }

    fn target_for_row_x(
        &self,
        rows: &[VisualRow],
        row_ix: usize,
        x: Pixels,
    ) -> (usize, CursorAffinity) {
        let row = &rows[row_ix];
        let y = row.y + row.height * 0.5;
        let index = self.closest_index_for_position(point(x, y));
        let affinity = if index == row.range.start {
            affinity_for_row_start(rows, row_ix)
        } else {
            CursorAffinity::Before
        };
        (index, affinity)
    }

    pub(super) fn closest_index_for_position(&self, position: Point<Pixels>) -> usize {
        if self.mixed.is_some() {
            return self.mixed_hit(position).0;
        }
        if self.lines.is_empty() {
            return 0;
        }
        if position.y < Pixels::ZERO {
            return 0;
        }
        if self.alignment_enabled {
            let row = &self.rows[row_for_y(&self.rows, position.y, self.line_height)];
            let line = &self.lines[row.logical_line];
            let x = position.x - row.x + row.start_x;
            let local = if x <= row.start_x {
                row.range.start - row.logical_start
            } else if x >= row.start_x + row.width {
                row.range.end - row.logical_start
            } else {
                line.unwrapped_layout.closest_index_for_x(x)
            };
            return (row.logical_start + local).clamp(row.range.start, row.range.end);
        }

        let mut logical_start = 0usize;
        let mut y = Pixels::ZERO;
        for line in &self.lines {
            let line_size = line.size(self.line_height);
            let bottom = y + line_size.height;
            if position.y <= bottom {
                let row_ix = row_for_y(&self.rows, position.y, self.line_height);
                let local_position = point(position.x - self.rows[row_ix].x, position.y - y);
                let local = line
                    .closest_index_for_position(local_position, self.line_height)
                    .unwrap_or_else(|edge| edge);
                return logical_start + local;
            }
            y = bottom;
            logical_start += line.len().saturating_add(1);
        }

        logical_start.saturating_sub(1)
    }

    pub(crate) fn caret_for_position(&self, position: Point<Pixels>) -> (usize, CursorAffinity) {
        if self.mixed.is_some() {
            return self.mixed_hit(position);
        }
        let rows = self.visual_rows();
        let row_ix = row_for_y(rows, position.y, self.line_height);
        let index = self.closest_index_for_position(position);
        let affinity = if index == rows[row_ix].range.start {
            affinity_for_row_start(rows, row_ix)
        } else {
            CursorAffinity::Before
        };
        (index, affinity)
    }

    fn visual_rows(&self) -> &[VisualRow] {
        &self.rows
    }

    fn measure_rows(&self) -> Vec<VisualRow> {
        let mut rows = Vec::new();
        let mut logical_start = 0usize;
        let mut y = Pixels::ZERO;

        for (logical_line, line) in self.lines.iter().enumerate() {
            let mut row_start = 0usize;
            let mut start_x = Pixels::ZERO;
            for boundary in line.wrap_boundaries() {
                let run = &line.runs()[boundary.run_ix];
                let glyph = &run.glyphs[boundary.glyph_ix];
                let row_end = glyph.index;
                rows.push(VisualRow {
                    range: logical_start + row_start..logical_start + row_end,
                    y,
                    height: self.line_height,
                    width: glyph.position.x - start_x,
                    x: Pixels::ZERO,
                    logical_line,
                    logical_start,
                    start_x,
                });
                row_start = row_end;
                start_x = glyph.position.x;
                y += self.line_height;
            }
            rows.push(VisualRow {
                range: logical_start + row_start..logical_start + line.len(),
                y,
                height: self.line_height,
                width: line.unwrapped_layout.width - start_x,
                x: Pixels::ZERO,
                logical_line,
                logical_start,
                start_x,
            });
            y += self.line_height;
            logical_start += line.len().saturating_add(1);
        }

        if rows.is_empty() {
            rows.push(VisualRow {
                range: 0..0,
                y: Pixels::ZERO,
                height: self.line_height,
                width: Pixels::ZERO,
                x: Pixels::ZERO,
                logical_line: 0,
                logical_start: 0,
                start_x: Pixels::ZERO,
            });
        }
        rows
    }
}

impl super::ParagraphView {
    pub(crate) fn visual_caret_x(&self, index: usize, affinity: CursorAffinity) -> Option<Pixels> {
        self.last_layout.as_ref()?.caret_x(index, affinity)
    }

    pub(crate) fn visual_vertical_target(
        &self,
        index: usize,
        affinity: CursorAffinity,
        desired_x: Pixels,
        down: bool,
    ) -> Option<(usize, CursorAffinity)> {
        self.last_layout
            .as_ref()?
            .vertical_target(index, affinity, desired_x, down)
    }

    pub(crate) fn visual_edge_row_target(
        &self,
        desired_x: Pixels,
        last: bool,
    ) -> Option<(usize, CursorAffinity)> {
        self.last_layout.as_ref()?.edge_row_target(desired_x, last)
    }

    pub(crate) fn visual_line_edge_target(
        &self,
        index: usize,
        affinity: CursorAffinity,
        to_end: bool,
    ) -> Option<(usize, CursorAffinity)> {
        self.last_layout
            .as_ref()?
            .visual_line_edge(index, affinity, to_end)
    }

    pub(crate) fn visual_is_soft_wrap_boundary(&self, index: usize) -> bool {
        self.last_layout
            .as_ref()
            .is_some_and(|layout| layout.is_soft_wrap_boundary(index))
    }

    pub(crate) fn hit_test_caret_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<(usize, CursorAffinity)> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        if !layout.is_available() {
            return None;
        }
        Some(
            layout.caret_for_position(point(position.x - bounds.left(), position.y - bounds.top())),
        )
    }

    pub(crate) fn focus_caret(&self) -> Option<(usize, CursorAffinity)> {
        let session = self.session.borrow();
        match session.selection().focus() {
            xiaomu_runtime::session::DocumentPosition::Inline(point)
                if point.node_id() == self.node =>
            {
                Some((point.text_offset().as_usize(), point.affinity()))
            }
            _ => None,
        }
    }
}

fn row_for_caret(rows: &[VisualRow], index: usize, affinity: CursorAffinity) -> Option<usize> {
    if affinity.is_after() {
        for row_ix in 1..rows.len() {
            if rows[row_ix].range.start == index && rows[row_ix - 1].range.end == index {
                return Some(row_ix);
            }
        }
    }

    rows.iter()
        .position(|row| index >= row.range.start && index <= row.range.end)
}

fn affinity_for_row_start(rows: &[VisualRow], row_ix: usize) -> CursorAffinity {
    if row_ix > 0 && rows[row_ix - 1].range.end == rows[row_ix].range.start {
        CursorAffinity::After
    } else {
        CursorAffinity::Before
    }
}

fn row_for_y(rows: &[VisualRow], y: Pixels, line_height: Pixels) -> usize {
    if y <= Pixels::ZERO {
        return 0;
    }
    let raw = (f32::from(y) / f32::from(line_height)).floor() as usize;
    raw.min(rows.len().saturating_sub(1))
}

#[derive(Clone, Debug)]
struct VisualRow {
    range: Range<usize>,
    y: Pixels,
    height: Pixels,
    width: Pixels,
    x: Pixels,
    logical_line: usize,
    logical_start: usize,
    start_x: Pixels,
}
