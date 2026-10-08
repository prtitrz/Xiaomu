//! Reading decorations and geometry consume the same measured display layout.
use super::{ParagraphView, layout::BlockTextLayout};
use crate::document_view::{ReadingRange, ReadingViewSnapshot, reading::SharedReadingState};
use gpui::{Bounds, PaintQuad, Pixels, Point, fill, px, rgba};
use xiaomu_core::selection::InlinePoint;

impl ParagraphView {
    pub(crate) fn attach_reading_state(&mut self, state: SharedReadingState) {
        self.reading = Some(state);
    }

    pub(super) fn reading_highlight_quads(
        &self,
        layout: &BlockTextLayout,
        bounds: Bounds<Pixels>,
        clip: Bounds<Pixels>,
    ) -> Vec<PaintQuad> {
        if self.is_composing() {
            return Vec::new();
        }
        let Some(state) = &self.reading else {
            return Vec::new();
        };
        let state = state.borrow();
        let session = self.session.borrow();
        if !state
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.matches(&state, &session))
        {
            return Vec::new();
        }
        let Some(ranges) = state.highlights.get(&self.node) else {
            return Vec::new();
        };
        let Some(projection) = self.atom_display_projection() else {
            return Vec::new();
        };
        let local_clip = Bounds::new(clip.origin - bounds.origin, clip.size);
        let Some(visible) = layout.reading_display_range(&local_clip) else {
            return Vec::new();
        };
        let edge = |byte, after| {
            projection
                .inline_point_for_display_boundary(
                    byte,
                    xiaomu_core::selection::CursorAffinity::Before,
                )
                .or_else(|| {
                    let atom = projection.atom_at_display_offset(byte)?;
                    Some(InlinePoint::new(
                        self.node,
                        atom.text_offset(),
                        atom.atom_index() + usize::from(after),
                        xiaomu_core::selection::CursorAffinity::Before,
                    ))
                })
        };
        let Some((start, end)) = edge(visible.start, false).zip(edge(visible.end, true)) else {
            return Vec::new();
        };
        let key = |point: InlinePoint| (point.text_offset(), point.atom_index());
        ranges
            .iter()
            .filter(|(range, _)| key(range.end()) > key(start) && key(range.start()) < key(end))
            .flat_map(|(range, active)| {
                let start = projection.display_offset_for_inline_point(range.start());
                let end = projection.display_offset_for_inline_point(range.end());
                start
                    .zip(end)
                    .into_iter()
                    .flat_map(|(start, end)| {
                        layout.reading_selection_rects(start..end, &local_clip)
                    })
                    .map(move |mut rect| {
                        rect.origin += bounds.origin;
                        fill(rect, rgba(if *active { 0xf59e0b88 } else { 0xfacc154d }))
                    })
            })
            .collect()
    }

    pub(super) fn record_reading_measurement(&mut self) {
        self.reading_measurement =
            self.reading
                .as_ref()
                .filter(|_| !self.is_composing())
                .map(|state| {
                    (
                        state.borrow().snapshot(&self.session.borrow()),
                        self.epoch.get(),
                    )
                });
    }

    pub(crate) fn reading_bounds(
        &self,
        snapshot: &ReadingViewSnapshot,
        range: ReadingRange,
    ) -> Option<Bounds<Pixels>> {
        self.current_reading_layout(snapshot)?;
        let projection = self.atom_display_projection()?;
        let start = projection.display_offset_for_inline_point(range.start())?;
        let end = projection.display_offset_for_inline_point(range.end())?;
        let layout = self.last_layout.as_ref()?;
        let mut rect = layout
            .selection_rects(start..end)
            .into_iter()
            .next()
            .or_else(|| layout.caret_rect(start, range.start().affinity(), px(2.0)))?;
        rect.origin += self.last_bounds?.origin;
        Some(rect)
    }

    pub(crate) fn reading_caret_bounds(
        &self,
        snapshot: &ReadingViewSnapshot,
        point: InlinePoint,
    ) -> Option<Bounds<Pixels>> {
        let mut bounds = self.reading_bounds(snapshot, ReadingRange::new(point, point))?;
        if let Some(height) = self.reading_caret_height(point).ok()? {
            bounds.origin.y += (bounds.size.height - height) / 2.0;
            bounds.size.height = height;
        }
        Some(bounds)
    }

    pub(crate) fn reading_hit(
        &self,
        snapshot: &ReadingViewSnapshot,
        position: Point<Pixels>,
    ) -> Option<InlinePoint> {
        self.current_reading_layout(snapshot)?;
        let (display, affinity) = self.hit_test_caret_position(position)?;
        self.atom_display_projection()?
            .inline_point_for_display_hit(display, affinity)
    }

    fn current_reading_layout(&self, snapshot: &ReadingViewSnapshot) -> Option<()> {
        let state = self.reading.as_ref()?.borrow();
        let session = self.session.borrow();
        if self.is_composing()
            || !snapshot.matches(&state, &session)
            || !self
                .reading_measurement
                .as_ref()?
                .0
                .matches(&state, &session)
            || self.reading_measurement.as_ref()?.1 != self.epoch.get()
            || !self.last_layout.as_ref()?.is_available()
            || self.cache_key.is_none()
        {
            return None;
        }
        Some(())
    }
}

#[cfg(test)]
#[path = "reading_tests.rs"]
mod tests;
