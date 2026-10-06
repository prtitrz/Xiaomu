//! Shared production dispatch for real-size glyphs and all editor coordinates.

use super::{BlockTextLayout, VisualRow, row_for_caret};
use crate::mixed_size::{Layout, MixedLayout};
use gpui::{App, Bounds, Pixels, Point, Window, point, px, size};
use std::ops::Range;
use std::rc::Rc;
use xiaomu_core::selection::CursorAffinity;

impl BlockTextLayout {
    pub(in crate::block_view) fn from_sized(layout: Layout) -> Self {
        match layout {
            Layout::Uniform(layout) => {
                let mut result = Self::new(layout.lines, layout.line_height);
                result.sized_font_size = Some(layout.font_size);
                result
            }
            Layout::Mixed(mixed) => Self::from_mixed(mixed),
        }
    }

    fn from_mixed(mixed: MixedLayout) -> Self {
        let mut layout = Self::new(Vec::new(), mixed.rows[0].height);
        layout.size = mixed.size;
        layout.rows = mixed
            .rows
            .iter()
            .map(|row| VisualRow {
                range: row.range.clone(),
                y: row.y,
                height: row.height,
                width: row.width,
                x: px(0.0),
                logical_line: 0,
                logical_start: 0,
                start_x: px(0.0),
            })
            .collect();
        layout.mixed = Some(Rc::new(mixed));
        layout
    }

    pub(in crate::block_view) fn sized_font_size(&self) -> Option<Pixels> {
        self.sized_font_size
    }

    pub(in crate::block_view) fn unavailable(line_height: Pixels) -> Self {
        let mut layout = Self::new(Vec::new(), line_height);
        layout.unavailable = true;
        layout
    }

    pub(in crate::block_view) fn is_available(&self) -> bool {
        !self.unavailable
    }

    pub(super) fn mixed_position(
        &self,
        index: usize,
        affinity: CursorAffinity,
    ) -> Option<Point<Pixels>> {
        let row_ix = row_for_caret(&self.rows, index, affinity)?;
        let mut position = self.mixed.as_ref()?.position_for_index(index, affinity)?;
        position.x += self.rows[row_ix].x;
        Some(position)
    }

    pub(super) fn mixed_hit(&self, position: Point<Pixels>) -> (usize, CursorAffinity) {
        let mixed = self.mixed.as_ref().expect("mixed dispatch");
        let row_ix = self
            .rows
            .iter()
            .position(|row| position.y < row.y + row.height)
            .unwrap_or(self.rows.len() - 1);
        let hit = mixed
            .closest_index_for_point(point(position.x - self.rows[row_ix].x, position.y))
            .expect("every mixed row has an edge");
        (hit.index, hit.affinity)
    }

    pub(super) fn mixed_selection_rects(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        self.mixed
            .as_ref()
            .expect("mixed dispatch")
            .selection_rects(range.start, range.end)
            .into_iter()
            .map(|mut rect| {
                if let Some(row) = self.rows.iter().find(|row| row.y == rect.origin.y) {
                    rect.origin.x += row.x;
                }
                rect.size.width = rect.size.width.max(px(1.0));
                rect
            })
            .collect()
    }

    /// A caret/native collapsed range uses the actual row, including mixed
    /// soft-wrap affinity. This is a line-box caret, not a product size policy.
    pub(in crate::block_view) fn caret_rect(
        &self,
        index: usize,
        affinity: CursorAffinity,
        width: Pixels,
    ) -> Option<Bounds<Pixels>> {
        if self.unavailable {
            return None;
        }
        let row = &self.rows[row_for_caret(&self.rows, index, affinity)?];
        if let Some(mixed) = &self.mixed {
            let mut rect = mixed.caret_rect(index, affinity, width)?;
            rect.origin.x += row.x;
            return Some(rect);
        }
        Some(Bounds::new(
            self.position_for_caret(index, affinity)?,
            size(width, row.height),
        ))
    }

    pub(in crate::block_view) fn paint_mixed(
        &self,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Result<()> {
        let Some(mixed) = self.mixed.as_ref() else {
            return Ok(());
        };
        mixed.paint_with_offsets(origin, |index| self.rows[index].x, window, cx)
    }
}
