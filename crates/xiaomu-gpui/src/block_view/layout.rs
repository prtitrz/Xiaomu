//! Wrapped text geometry for one inline-bearing block.
//!
//! This layer is deliberately GPUI-local. Canonical positions remain Core
//! [`xiaomu_core::selection::TextPoint`] values; soft-wrap only projects byte
//! offsets into visual rows and pixels.

use std::ops::Range;

use gpui::{Bounds, Pixels, Point, Size, WrappedLine, point, px, size};
use xiaomu_core::selection::CursorAffinity;

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
    line_height: Pixels,
    size: Size<Pixels>,
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
        Self {
            lines,
            line_height,
            size: measured,
        }
    }

    pub(super) fn size(&self) -> Size<Pixels> {
        self.size
    }

    pub(super) fn line_height(&self) -> Pixels {
        self.line_height
    }

    pub(super) fn lines(&self) -> &[WrappedLine] {
        &self.lines
    }

    pub(super) fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        let mut logical_start = 0usize;
        let mut y = Pixels::ZERO;

        for line in &self.lines {
            let logical_end = logical_start + line.len();
            if index <= logical_end {
                let local = index - logical_start;
                return line
                    .position_for_index(local, self.line_height)
                    .map(|position| point(position.x, position.y + y));
            }
            logical_start = logical_end.saturating_add(1);
            y += line.size(self.line_height).height;
        }

        if self.lines.is_empty() && index == 0 {
            Some(point(Pixels::ZERO, Pixels::ZERO))
        } else {
            None
        }
    }

    pub(crate) fn position_for_caret(
        &self,
        index: usize,
        affinity: CursorAffinity,
    ) -> Option<Point<Pixels>> {
        let rows = self.visual_rows();
        let row_ix = row_for_caret(&rows, index, affinity)?;
        let row = &rows[row_ix];

        if affinity.is_after()
            && row_ix > 0
            && row.range.start == index
            && rows[row_ix - 1].range.end == index
        {
            return Some(point(Pixels::ZERO, row.y));
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
        let current = row_for_caret(&rows, index, affinity)?;
        let target = if down {
            current
                .checked_add(1)
                .filter(|target| *target < rows.len())?
        } else {
            current.checked_sub(1)?
        };
        Some(self.target_for_row_x(&rows, target, desired_x))
    }

    pub(crate) fn edge_row_target(
        &self,
        desired_x: Pixels,
        last: bool,
    ) -> Option<(usize, CursorAffinity)> {
        let rows = self.visual_rows();
        let row_ix = if last { rows.len().checked_sub(1)? } else { 0 };
        Some(self.target_for_row_x(&rows, row_ix, desired_x))
    }

    pub(crate) fn visual_line_edge(
        &self,
        index: usize,
        affinity: CursorAffinity,
        to_end: bool,
    ) -> Option<(usize, CursorAffinity)> {
        let rows = self.visual_rows();
        let row_ix = row_for_caret(&rows, index, affinity)?;
        let row = &rows[row_ix];
        if to_end {
            Some((row.range.end, CursorAffinity::Before))
        } else {
            Some((row.range.start, affinity_for_row_start(&rows, row_ix)))
        }
    }

    fn target_for_row_x(
        &self,
        rows: &[VisualRow],
        row_ix: usize,
        x: Pixels,
    ) -> (usize, CursorAffinity) {
        let row = &rows[row_ix];
        let y = row.y + self.line_height * 0.5;
        let index = self.closest_index_for_position(point(x, y));
        let affinity = if index == row.range.start {
            affinity_for_row_start(rows, row_ix)
        } else {
            CursorAffinity::Before
        };
        (index, affinity)
    }

    pub(super) fn closest_index_for_position(&self, position: Point<Pixels>) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        if position.y < Pixels::ZERO {
            return 0;
        }

        let mut logical_start = 0usize;
        let mut y = Pixels::ZERO;
        for line in &self.lines {
            let line_size = line.size(self.line_height);
            let bottom = y + line_size.height;
            if position.y <= bottom {
                let local_position = point(position.x, position.y - y);
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
        let rows = self.visual_rows();
        let row_ix = row_for_y(&rows, position.y, self.line_height);
        let index = self.closest_index_for_position(position);
        let affinity = if index == rows[row_ix].range.start {
            affinity_for_row_start(&rows, row_ix)
        } else {
            CursorAffinity::Before
        };
        (index, affinity)
    }

    pub(super) fn selection_rects(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        if range.start >= range.end {
            return Vec::new();
        }

        let mut rects = Vec::new();
        for visual in self.visual_rows() {
            let start = range.start.max(visual.range.start);
            let end = range.end.min(visual.range.end);
            if start >= end {
                continue;
            }

            let start_x = if start == visual.range.start {
                Pixels::ZERO
            } else {
                self.position_for_index(start)
                    .map(|position| position.x)
                    .unwrap_or(Pixels::ZERO)
            };
            let end_x = self
                .position_for_index(end)
                .map(|position| position.x)
                .unwrap_or(start_x);
            let left = start_x.min(end_x);
            let width = (start_x.max(end_x) - left).max(px(1.0));
            rects.push(Bounds::new(
                point(left, visual.y),
                size(width, self.line_height),
            ));
        }
        rects
    }

    fn visual_rows(&self) -> Vec<VisualRow> {
        let mut rows = Vec::new();
        let mut logical_start = 0usize;
        let mut y = Pixels::ZERO;

        for line in &self.lines {
            let mut row_start = 0usize;
            for boundary in line.wrap_boundaries() {
                let run = &line.runs()[boundary.run_ix];
                let row_end = run.glyphs[boundary.glyph_ix].index;
                rows.push(VisualRow {
                    range: logical_start + row_start..logical_start + row_end,
                    y,
                });
                row_start = row_end;
                y += self.line_height;
            }
            rows.push(VisualRow {
                range: logical_start + row_start..logical_start + line.len(),
                y,
            });
            y += self.line_height;
            logical_start += line.len().saturating_add(1);
        }

        if rows.is_empty() {
            rows.push(VisualRow {
                range: 0..0,
                y: Pixels::ZERO,
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
        Some(
            layout.caret_for_position(point(position.x - bounds.left(), position.y - bounds.top())),
        )
    }

    pub(crate) fn focus_caret(&self) -> Option<(usize, CursorAffinity)> {
        let session = self.session.borrow();
        match session.selection().focus() {
            xiaomu_runtime::session::DocumentPosition::Text(point)
                if point.node_id() == self.node =>
            {
                Some((point.offset().as_usize(), point.affinity()))
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
}
