//! Coherent, guarded scroll plans across nested measured horizontal viewports.
use super::{DocumentView, ReadingRevealStatus};
use gpui::{Bounds, Context, Pixels, Point, ScrollHandle, Window, point, px};
use xiaomu_core::document::NodeKind;

#[derive(Clone)]
struct ScrollChange {
    handle: ScrollHandle,
    bounds: Bounds<Pixels>,
    maximum: gpui::Size<Pixels>,
    before: Point<Pixels>,
    after: Point<Pixels>,
}

impl DocumentView {
    pub(super) fn finish_reading_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.record_reading_geometry(window);
        let (pending, frame) = {
            let state = self.reading.borrow();
            if state.scheduled {
                return;
            }
            let Some(pending) = &state.pending else {
                return;
            };
            (pending.clone(), state.frame)
        };
        if self.validate_reading_snapshot(&pending.snapshot).is_err() {
            self.reading.borrow_mut().cancel();
            return;
        }
        let Some(mut target) = self.reading_target_bounds(&pending.snapshot, pending.range, cx)
        else {
            // Called only after a complete paint. A hidden/failed table or
            // unavailable text layout must not loop forever or claim success.
            let composing = self
                .children
                .iter()
                .find(|(node, _)| *node == pending.range.start().node_id())
                .is_some_and(|(_, child)| child.read(cx).is_composing());
            if !composing {
                self.finish_reading_reveal(ReadingRevealStatus::Unavailable);
            }
            return;
        };
        let mut changes = Vec::new();
        {
            let session = self.session.borrow();
            let document = session.document();
            let state = self.reading.borrow();
            let mut ancestor = Some(pending.range.start().node_id());
            while let Some(node) = ancestor {
                if document
                    .node(node)
                    .is_some_and(|node| node.kind() == &NodeKind::Table)
                    && self.table_capability.borrow().enabled()
                {
                    let Some(handle) = state.tables.get(&node) else {
                        return;
                    };
                    if handle.bounds().size.width <= px(0.0) {
                        return;
                    }
                    plan_scroll(handle, &mut target, true, false, &mut changes);
                }
                ancestor = document.parent_of(node);
            }
        }
        if self.scroll_handle.bounds().size.height <= px(0.0) {
            return;
        }
        plan_scroll(&self.scroll_handle, &mut target, true, true, &mut changes);
        if changes.iter().all(|change| change.before == change.after) {
            self.finish_reading_reveal(
                if self
                    .reading_visible_bounds(pending.range.start().node_id(), target)
                    .is_some()
                {
                    ReadingRevealStatus::Revealed
                } else {
                    ReadingRevealStatus::Unavailable
                },
            );
            return;
        }
        self.reading.borrow_mut().scheduled = true;
        let weak = cx.weak_entity();
        let viewport_size = window.viewport_size();
        let scale = window.scale_factor();
        window.defer(cx, move |window, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.reading.borrow_mut().scheduled = false;
                let current = view
                    .reading
                    .borrow()
                    .pending
                    .as_ref()
                    .is_some_and(|current| current.generation == pending.generation);
                if !current {
                    if view.reading.borrow().pending.is_some() {
                        cx.notify();
                    }
                    return;
                }
                if view.validate_reading_snapshot(&pending.snapshot).is_err() {
                    view.reading.borrow_mut().cancel();
                    return;
                }
                // User scroll supersedes a queued reveal rather than snapping
                // back. Layout/scale changes get another measured frame.
                if changes
                    .iter()
                    .any(|change| change.handle.offset() != change.before)
                {
                    view.reading.borrow_mut().cancel();
                    return;
                }
                if view.reading.borrow().frame != frame
                    || window.viewport_size() != viewport_size
                    || window.scale_factor() != scale
                    || changes.iter().any(|change| {
                        change.handle.bounds() != change.bounds
                            || change.handle.max_offset() != change.maximum
                    })
                {
                    cx.notify();
                    return;
                }
                for change in changes {
                    if change.before != change.after {
                        change.handle.set_offset(change.after);
                    }
                }
                // Keep pending until the scrolled frame confirms visibility.
                cx.notify();
            });
        });
    }

    fn finish_reading_reveal(&self, status: ReadingRevealStatus) {
        let mut state = self.reading.borrow_mut();
        state.pending = None;
        state.status = Some(status);
    }
}

fn axis_offset(
    start: Pixels,
    end: Pixels,
    view_start: Pixels,
    view_end: Pixels,
    offset: Pixels,
    maximum: Pixels,
) -> Pixels {
    let end = end.min(start + (view_end - view_start).max(px(0.0)));
    let adjustment = if start < view_start {
        view_start - start
    } else if end > view_end {
        view_end - end
    } else {
        px(0.0)
    };
    (offset + adjustment).clamp(-maximum.max(px(0.0)), px(0.0))
}

fn plan_scroll(
    handle: &ScrollHandle,
    target: &mut Bounds<Pixels>,
    horizontal: bool,
    vertical: bool,
    changes: &mut Vec<ScrollChange>,
) {
    let bounds = handle.bounds();
    let before = handle.offset();
    let max = handle.max_offset();
    let after = point(
        if horizontal {
            axis_offset(
                target.left(),
                target.right(),
                bounds.left(),
                bounds.right(),
                before.x,
                max.width,
            )
        } else {
            before.x
        },
        if vertical {
            axis_offset(
                target.top(),
                target.bottom(),
                bounds.top(),
                bounds.bottom(),
                before.y,
                max.height,
            )
        } else {
            before.y
        },
    );
    if horizontal {
        target.size.width = target.size.width.min(bounds.size.width);
    }
    if vertical {
        target.size.height = target.size.height.min(bounds.size.height);
    }
    target.origin += after - before;
    changes.push(ScrollChange {
        handle: handle.clone(),
        bounds,
        maximum: max,
        before,
        after,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimum_axis_motion_preserves_visible_and_clamps_oversized_targets() {
        assert_eq!(
            axis_offset(px(20.0), px(40.0), px(0.0), px(100.0), px(-30.0), px(500.0)),
            px(-30.0)
        );
        assert_eq!(
            axis_offset(
                px(120.0),
                px(160.0),
                px(0.0),
                px(100.0),
                px(-30.0),
                px(500.0)
            ),
            px(-90.0)
        );
        assert_eq!(
            axis_offset(
                px(-10.0),
                px(20.0),
                px(0.0),
                px(100.0),
                px(-30.0),
                px(500.0)
            ),
            px(-20.0)
        );
        assert_eq!(
            axis_offset(px(150.0), px(800.0), px(0.0), px(100.0), px(0.0), px(500.0)),
            px(-150.0)
        );
    }
}
