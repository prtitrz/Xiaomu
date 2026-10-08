//! Current caret and viewport-top reading positions from actual native layout.
use super::{DocumentView, ReadingRange, ReadingViewSnapshot};
use gpui::{App, Bounds, Pixels, Point, ScrollHandle, Size, Window, point, px};
use xiaomu_core::{
    document::{NodeId, NodeKind},
    selection::InlinePoint,
};
use xiaomu_runtime::session::DocumentPosition;

pub(super) struct ReadingGeometry {
    snapshot: ReadingViewSnapshot,
    viewport: Size<Pixels>,
    scale: f32,
    scrolls: Vec<ScrollObservation>,
}

struct ScrollObservation {
    handle: ScrollHandle,
    bounds: Bounds<Pixels>,
    offset: Point<Pixels>,
    maximum: Size<Pixels>,
}

impl DocumentView {
    /// Returns the visible ordered selection start, otherwise the position at
    /// the actual viewport top. No focus, selection, scroll or history changes.
    ///
    /// The result uses wrapped, mixed-size/aligned measured rows and every
    /// enclosing table clip. Returns `None` before measurement, during stale
    /// geometry, or when the viewport contains no accessible inline surface.
    #[must_use]
    pub fn reading_start(&self, window: &Window, cx: &App) -> Option<InlinePoint> {
        if !self.reading_geometry_current(window) {
            return None;
        }
        let snapshot = self.reading_snapshot();
        let selection_start = {
            let session = self.session.borrow();
            session.selection().ordered(session.document()).ok()?.0
        };
        if let DocumentPosition::Inline(caret) = selection_start
            && let Some(rect) = self
                .children
                .iter()
                .find(|(node, _)| *node == caret.node_id())
                .and_then(|(_, child)| child.read(cx).reading_caret_bounds(&snapshot, caret))
            && self.reading_visible_bounds(caret.node_id(), rect).is_some()
        {
            return Some(caret);
        }
        let viewport = self.scroll_handle.bounds();
        let registry = self.registry.borrow();
        let mut best = None;
        for (node, bounds) in registry.iter() {
            let Some(visible) = self.reading_visible_bounds(*node, *bounds) else {
                continue;
            };
            let score = (visible.top(), visible.left());
            if best
                .as_ref()
                .is_some_and(|(_, previous)| *previous <= score)
            {
                continue;
            }
            let Some((_, child)) = self.children.iter().find(|(id, _)| id == node) else {
                continue;
            };
            // Use the real visible leading edge. Hit-testing resolves a partial
            // top row, alignment inset and atom renderer span to exact points.
            if let Some(position) = child.read(cx).reading_hit(
                &snapshot,
                point(visible.left(), viewport.top().max(visible.top())),
            ) {
                best = Some((position, score));
            }
        }
        best.map(|(position, _)| position)
    }

    pub(super) fn record_reading_geometry(&self, window: &Window) {
        let mut state = self.reading.borrow_mut();
        state.geometry = Some(ReadingGeometry {
            snapshot: state.snapshot(&self.session.borrow()),
            viewport: window.viewport_size(),
            scale: window.scale_factor(),
            scrolls: std::iter::once(&self.scroll_handle)
                .chain(state.tables.values())
                .map(|handle| ScrollObservation {
                    handle: handle.clone(),
                    bounds: handle.bounds(),
                    offset: handle.offset(),
                    maximum: handle.max_offset(),
                })
                .collect(),
        });
    }

    fn reading_geometry_current(&self, window: &Window) -> bool {
        let state = self.reading.borrow();
        state.geometry.as_ref().is_some_and(|geometry| {
            geometry.snapshot.matches(&state, &self.session.borrow())
                && geometry.viewport == window.viewport_size()
                && geometry.scale == window.scale_factor()
                && geometry.scrolls.iter().all(|observed| {
                    observed.handle.bounds() == observed.bounds
                        && observed.handle.offset() == observed.offset
                        && observed.handle.max_offset() == observed.maximum
                })
        })
    }

    pub(super) fn reading_target_bounds(
        &self,
        snapshot: &ReadingViewSnapshot,
        range: ReadingRange,
        cx: &App,
    ) -> Option<Bounds<Pixels>> {
        let node = range.start().node_id();
        let session = self.session.borrow();
        if self
            .hidden_table_ancestor(session.document(), node)
            .is_some()
        {
            return None;
        }
        self.block_bounds(node)?;
        self.children
            .iter()
            .find(|(id, _)| *id == node)?
            .1
            .read(cx)
            .reading_bounds(snapshot, range)
    }

    pub(super) fn reading_visible_bounds(
        &self,
        node: NodeId,
        bounds: Bounds<Pixels>,
    ) -> Option<Bounds<Pixels>> {
        let session = self.session.borrow();
        let document = session.document();
        if self.hidden_table_ancestor(document, node).is_some() {
            return None;
        }
        let mut visible = bounds.intersect(&self.scroll_handle.bounds());
        let clips = self.table_clips.borrow();
        let mut ancestor = Some(node);
        while let Some(id) = ancestor {
            if document.node(id)?.kind() == &NodeKind::Table
                && self.table_capability.borrow().enabled()
            {
                // A missing table measurement is not an unobstructed path.
                visible = visible.intersect(clips.get(&id)?);
            }
            ancestor = document.parent_of(id);
        }
        (visible.size.width > px(0.0) && visible.size.height > px(0.0)).then_some(visible)
    }
}
