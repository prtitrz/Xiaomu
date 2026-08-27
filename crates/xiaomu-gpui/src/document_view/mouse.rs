//! Mouse hit-testing and drag selection for [`DocumentView`].
//!
//! Split out of `actions.rs` so P3 clipboard / history wiring can grow
//! without stacking onto the same file as pointer dispatch.

use gpui::{App, Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window};

use super::{DocumentView, navigation};
use xiaomu_core::document::NodeId;
use xiaomu_core::selection::{CursorAffinity, TextPoint};

impl DocumentView {
    // ---- mouse ----

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_dragging = true;
        if let Some(point) = self.hit_test(event.position, cx) {
            // Click-placement diagnostic: shows which block the click landed
            // on, so mis-hits are visible during real-machine testing.
            let clicked = {
                let session = self.session.borrow();
                session.document().node(point.node_id()).map(|node| {
                    let text = node
                        .content()
                        .as_inline()
                        .map(|inline| {
                            let text: String = inline
                                .runs()
                                .iter()
                                .map(|run| run.text().as_str())
                                .collect();
                            let preview: String = text.chars().take(8).collect();
                            format!(" \u{201c}{preview}\u{201d}")
                        })
                        .unwrap_or_default();
                    format!(
                        "{:?}{text} at byte {}",
                        node.kind(),
                        point.offset().as_usize()
                    )
                })
            };
            if let Some(description) = clicked {
                eprintln!("xiaomu: click placed caret in {description}");
            }
            if event.modifiers.shift {
                self.move_focus_to(point, true, window, cx);
            } else {
                self.place(point, window, cx);
            }
        }
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_dragging {
            return;
        }
        if let Some(point) = self.hit_test(event.position, cx) {
            self.move_focus_to(point, true, window, cx);
        }
    }

    pub(crate) fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_dragging = false;
    }

    /// Maps a window-space point to a validated caret point via the paint
    /// registry: nearest block by vertical position, then x hit-testing
    /// within that block's shaped line.
    fn hit_test(&self, position: Point<Pixels>, cx: &App) -> Option<TextPoint> {
        let registry = self.registry.borrow();
        let mut nearest: Option<(NodeId, Pixels)> = None;
        for (node, bounds) in registry.iter() {
            let distance = if position.y < bounds.top() {
                bounds.top() - position.y
            } else if position.y > bounds.bottom() {
                position.y - bounds.bottom()
            } else {
                Pixels::ZERO
            };
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((*node, distance));
            }
        }
        let (node, _) = nearest?;

        let session = self.session.borrow();
        let blocks = navigation::text_blocks(session.document());
        drop(session);
        let block = blocks.iter().find(|block| block.node == node)?;
        let child = self
            .children
            .iter()
            .find(|(id, _)| *id == node)
            .map(|(_, view)| view.clone())?;
        let raw = child.read(cx).hit_test_x(position.x)?;
        let clamped = raw.min(block.text().len());
        let offset = navigation::validated_offset(block, clamped)
            .or_else(|| navigation::validated_offset(block, block.text().len()))?;
        Some(TextPoint::new(node, offset, CursorAffinity::Before))
    }
}
