//! Wrapped text geometry for one inline-bearing block.
//!
//! This layer is deliberately GPUI-local. Canonical positions remain Core
//! [`xiaomu_core::selection::TextPoint`] values; soft-wrap only projects byte
//! offsets into visual rows and pixels.

use std::ops::Range;

use gpui::{Bounds, Pixels, Point, Size, WrappedLine, point, px, size};

/// Measured wrapped text for one block.
///
/// `WrappedLine` here means one logical line as understood by GPUI; each one
/// may itself contain several soft-wrapped visual rows. Paragraphs currently
/// contain one logical line, while keeping this representation multi-line
/// ready avoids rebuilding the geometry layer when CodeBlock gains newline
/// semantics later in P3.
#[derive(Clone, Debug)]
pub(super) struct BlockTextLayout {
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
        // An empty editable paragraph still needs one caret row.
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

    /// Maps a canonical/display byte index to a point relative to this block.
    ///
    /// At a soft-wrap boundary GPUI 0.2.2 resolves the shared logical index
    /// to the upstream visual row. P3.2 will use `CursorAffinity` to choose
    /// upstream/downstream explicitly; P3.1 keeps GPUI's deterministic base
    /// behavior while establishing the geometry path.
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
            logical_start = logical_end.saturating_add(1); // hard newline separator
            y += line.size(self.line_height).height;
        }

        if self.lines.is_empty() && index == 0 {
            Some(point(Pixels::ZERO, Pixels::ZERO))
        } else {
            None
        }
    }

    /// Maps a point relative to this block to the nearest byte index.
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

    /// Returns one relative selection rectangle per intersected visual row.
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

struct VisualRow {
    range: Range<usize>,
    y: Pixels,
}
