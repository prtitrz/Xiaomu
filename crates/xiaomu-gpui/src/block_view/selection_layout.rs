//! Shared selection/reading rectangles with direct measured-row addressing.
use super::{BlockTextLayout, VisualRow};
use gpui::{Bounds, Pixels, point, px, size};
use std::ops::Range;

#[cfg(test)]
std::thread_local! { static ROW_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

impl BlockTextLayout {
    #[cfg(test)]
    pub(in crate::block_view) fn reading_row_visits(reset: bool) -> usize {
        ROW_VISITS.with(|visits| {
            if reset {
                visits.replace(0)
            } else {
                visits.get()
            }
        })
    }

    pub(in crate::block_view) fn selection_rects(
        &self,
        range: Range<usize>,
    ) -> Vec<Bounds<Pixels>> {
        if let Some(mixed) = &self.mixed {
            return mixed
                .selection_rects(range.start, range.end)
                .into_iter()
                .map(|mut rect| {
                    let index = self.rows.partition_point(|row| row.y < rect.origin.y);
                    if let Some(row) = self.rows.get(index) {
                        rect.origin.x += row.x;
                    }
                    rect.size.width = rect.size.width.max(px(1.0));
                    rect
                })
                .collect();
        }
        self.selection_rects_in_rows(range, 0..self.rows.len())
    }

    /// A broad exact-byte window of the rows intersecting the current clip.
    pub(in crate::block_view) fn reading_display_range(
        &self,
        clip: &Bounds<Pixels>,
    ) -> Option<Range<usize>> {
        let rows = self.visible_rows(clip);
        let first = self.rows.get(rows.start)?;
        let last = self.rows.get(rows.end.checked_sub(1)?)?;
        if rows.is_empty() {
            return None;
        }
        let end = self
            .rows
            .get(rows.end)
            .map_or(last.range.end, |next| next.range.start.max(last.range.end));
        Some(first.range.start..end)
    }

    pub(in crate::block_view) fn reading_selection_rects(
        &self,
        range: Range<usize>,
        clip: &Bounds<Pixels>,
    ) -> Vec<Bounds<Pixels>> {
        self.selection_rects_in_rows(range, self.visible_rows(clip))
    }

    fn visible_rows(&self, clip: &Bounds<Pixels>) -> Range<usize> {
        if clip.size.width <= px(0.0) || clip.size.height <= px(0.0) {
            return 0..0;
        }
        let start = self
            .rows
            .partition_point(|row| row.y + row.height <= clip.top());
        let end = self.rows.partition_point(|row| row.y < clip.bottom());
        start..end.max(start)
    }

    fn selection_rects_in_rows(
        &self,
        range: Range<usize>,
        rows: Range<usize>,
    ) -> Vec<Bounds<Pixels>> {
        if self.unavailable || range.start >= range.end {
            return Vec::new();
        }
        let start = self
            .rows
            .partition_point(|row| row.range.end < range.start)
            .max(rows.start);
        let end = self
            .rows
            .partition_point(|row| row.range.start < range.end)
            .min(rows.end);
        let mut rects = Vec::new();
        let mut newlines = Vec::new();
        for index in start..end {
            #[cfg(test)]
            ROW_VISITS.with(|visits| visits.set(visits.get() + 1));
            let visual = &self.rows[index];
            if let Some(mixed) = &self.mixed {
                if let Some(mut rect) = mixed.rows[index].selection_rect(&range) {
                    rect.origin.x += visual.x;
                    rect.size.width = rect.size.width.max(px(1.0));
                    rects.push(rect);
                }
                continue;
            }
            let from = range.start.max(visual.range.start);
            let to = range.end.min(visual.range.end);
            if from < to {
                let left = self.selection_x(visual, from);
                let right = self.selection_x(visual, to);
                rects.push(Bounds::new(
                    point(left.min(right), visual.y),
                    size((right - left).abs().max(px(1.0)), self.line_height),
                ));
            }
            // Only the final visual row of a nonfinal logical line owns LF.
            let line = &self.lines[visual.logical_line];
            let newline = visual.logical_start + line.len();
            if visual.range.end == newline
                && visual.logical_line + 1 < self.lines.len()
                && range.start <= newline
                && newline < range.end
            {
                newlines.push(Bounds::new(
                    point(self.selection_x(visual, newline), visual.y),
                    size(px(4.0), self.line_height),
                ));
            }
        }
        rects.extend(newlines);
        rects
    }

    fn selection_x(&self, visual: &VisualRow, index: usize) -> Pixels {
        if !self.alignment_enabled && index == visual.range.start {
            return visual.x;
        }
        let positions = self.selection_positions.get_or_init(|| {
            self.lines
                .iter()
                .map(|line| {
                    let mut positions = Vec::new();
                    // Preserve GPUI x_for_index's first-in-render-order >= index
                    // rule even if bidi glyph indices are not monotonically sorted.
                    for glyph in line.runs().iter().flat_map(|run| &run.glyphs) {
                        if positions
                            .last()
                            .is_none_or(|(index, _)| glyph.index > *index)
                        {
                            positions.push((glyph.index, glyph.position.x));
                        }
                    }
                    positions
                })
                .collect()
        });
        let positions = &positions[visual.logical_line];
        let line = &self.lines[visual.logical_line];
        let x = |index| {
            positions
                .get(positions.partition_point(|(end, _)| *end < index))
                .map_or(line.unwrapped_layout.width, |(_, x)| *x)
        };
        let start = if self.alignment_enabled {
            visual.start_x
        } else {
            x(visual.range.start - visual.logical_start)
        };
        visual.x + x(index - visual.logical_start) - start
    }
}
