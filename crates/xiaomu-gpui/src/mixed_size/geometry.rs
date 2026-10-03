use super::{Hit, MixedLayout, Row};
use gpui::{App, Bounds, Pixels, Point, Window, point, px, size};
use std::ops::Range;
use xiaomu_core::selection::CursorAffinity;

impl MixedLayout {
    /// Locate every UTF-8 scalar boundary, including grapheme/ligature interiors
    /// that a platform UTF-16 range can address. Only invalid UTF-8 byte offsets
    /// return `None`. Stock GPUI's x_for_index can map multiple offsets to the
    /// same x; these are native mappings, not invented GDEF caret metrics.
    /// Fragment starts are normalized to advance origins, matching hit stops.
    /// `Before` / `After` select the preceding / following soft-wrapped row.
    pub(crate) fn position_for_index(
        &self,
        index: usize,
        affinity: CursorAffinity,
    ) -> Option<Point<Pixels>> {
        let row = self.visual_row(index, affinity)?;
        row.carets
            .binary_search_by_key(&index, |stop| stop.index)
            .ok()
            .map(|offset| point(row.carets[offset].x, row.y))
    }

    pub(crate) fn visual_row(&self, index: usize, affinity: CursorAffinity) -> Option<&Row> {
        let matches = |row: &&Row| {
            index >= row.range.start
                && index <= row.range.end
                && row
                    .carets
                    .binary_search_by_key(&index, |stop| stop.index)
                    .is_ok()
        };
        match affinity {
            CursorAffinity::Before => self.rows.iter().find(matches),
            CursorAffinity::After => self.rows.iter().rev().find(matches),
        }
    }

    pub(crate) fn caret_rect(
        &self,
        index: usize,
        affinity: CursorAffinity,
        width: Pixels,
    ) -> Option<Bounds<Pixels>> {
        let row = self.visual_row(index, affinity)?;
        let position = self.position_for_index(index, affinity)?;
        Some(Bounds::new(position, size(width.max(px(0.0)), row.height)))
    }

    /// Y is searched against actual cumulative row heights, never divided by a
    /// global line height. Out-of-bounds coordinates clamp to the nearest row.
    pub(crate) fn row_for_y(&self, y: Pixels) -> Option<&Row> {
        self.rows
            .iter()
            .find(|row| y < row.y + row.height)
            .or_else(|| self.rows.last())
    }

    /// Nearest safe cluster edge, used for pointer hit testing and desired-x
    /// vertical movement. At a soft boundary the hit retains visual affinity.
    pub(crate) fn closest_index_for_point(&self, position: Point<Pixels>) -> Option<Hit> {
        let row = self.row_for_y(position.y)?;
        let mut closest = row.stops.first()?;
        for stop in &row.stops[1..] {
            if f32::from(stop.x - position.x).abs() < f32::from(closest.x - position.x).abs() {
                closest = stop;
            }
        }
        let affinity = if closest.index == row.range.start {
            CursorAffinity::After
        } else {
            CursorAffinity::Before
        };
        Some(Hit {
            index: closest.index,
            affinity,
        })
    }

    /// Scalar-boundary rectangles for either selection direction. Invalid UTF-8
    /// bytes expand to adjacent scalar boundaries. Cluster interiors follow GPUI
    /// and may yield a zero-width rectangle (no internal GDEF caret metrics).
    /// LF selection gets a one-pixel continuation marker on its preceding row.
    pub(crate) fn selection_rects(&self, anchor: usize, head: usize) -> Vec<Bounds<Pixels>> {
        let selection = anchor.min(head).min(self.text_len)..anchor.max(head).min(self.text_len);
        if selection.is_empty() {
            return Vec::new();
        }
        self.rows
            .iter()
            .filter_map(|row| row.selection_rect(&selection))
            .collect()
    }

    /// A single bounding box for a platform/IME range. Both endpoints must
    /// be valid UTF-8 scalar positions. Stock GPUI can collapse ligature
    /// interiors to the same x; keep a caret-width box in that case.
    pub(crate) fn bounds_for_range(
        &self,
        anchor: usize,
        head: usize,
        affinity: CursorAffinity,
        caret_width: Pixels,
    ) -> Option<Bounds<Pixels>> {
        self.visual_row(anchor, affinity)?;
        self.visual_row(head, affinity)?;
        if anchor == head {
            return self.caret_rect(anchor, affinity, caret_width);
        }
        let rects = self.selection_rects(anchor, head);
        let first = rects.first()?;
        let mut left = first.origin.x;
        let mut top = first.origin.y;
        let mut right = left + first.size.width;
        let mut bottom = top + first.size.height;
        for rect in &rects[1..] {
            left = left.min(rect.origin.x);
            top = top.min(rect.origin.y);
            right = right.max(rect.origin.x + rect.size.width);
            bottom = bottom.max(rect.origin.y + rect.size.height);
        }
        Some(Bounds::new(
            point(left, top),
            size((right - left).max(caret_width.max(px(0.0))), bottom - top),
        ))
    }

    /// Paint the same final-sized fragments measured above. Each line contains
    /// only its row bytes, so no glyph clipping or repeated whole-span painting
    /// is involved. The offset cancels GPUI's per-line centering to share the
    /// row baseline; line metrics, decorations and native glyph rasterization
    /// remain GPUI's. Call only from the host element's normal paint phase.
    pub(crate) fn paint(
        &self,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Result<()> {
        // All backgrounds precede all glyphs: a following fragment's fill
        // must not erase an earlier italic glyph's advance-box overhang.
        for row in &self.rows {
            for fragment in &row.fragments {
                let (fragment_origin, height) = row.fragment_paint_geometry(fragment, origin);
                fragment
                    .line
                    .paint_background(fragment_origin, height, window, cx)?;
            }
        }
        for row in &self.rows {
            for fragment in &row.fragments {
                let (fragment_origin, height) = row.fragment_paint_geometry(fragment, origin);
                fragment.line.paint(fragment_origin, height, window, cx)?;
            }
        }
        Ok(())
    }
}

impl Row {
    pub(super) fn fragment_paint_geometry(
        &self,
        fragment: &super::Fragment,
        origin: Point<Pixels>,
    ) -> (Point<Pixels>, Pixels) {
        let natural_height = fragment.line.ascent + fragment.line.descent;
        (
            point(
                origin.x + fragment.x,
                origin.y + self.y + self.baseline - fragment.line.ascent,
            ),
            natural_height,
        )
    }

    fn selection_rect(&self, selection: &Range<usize>) -> Option<Bounds<Pixels>> {
        let start = selection.start.max(self.range.start);
        let end = selection.end.min(self.range.end);
        let newline_selected = self.hard_break_after
            && selection.start <= self.range.end
            && selection.end > self.range.end;
        if start >= end && !newline_selected {
            return None;
        }
        let left = self
            .carets
            .iter()
            .rev()
            .find(|stop| stop.index <= start)
            .map_or(px(0.0), |stop| stop.x);
        let mut right = self
            .carets
            .iter()
            .find(|stop| stop.index >= end)
            .map_or(self.width, |stop| stop.x);
        if newline_selected {
            right = right.max(self.width + px(1.0));
        }
        Some(Bounds::new(
            point(left, self.y),
            size((right - left).max(px(0.0)), self.height),
        ))
    }
}
