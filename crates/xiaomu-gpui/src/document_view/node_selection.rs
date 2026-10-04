//! Explicit whole-block focus, presentation and opt-in navigation.
//! Runtime owns identity and validation; no endpoint projection recreates it.

use gpui::{Context, Focusable as _, Window, canvas, div, prelude::*, px};
use xiaomu_core::document::NodeId;
use xiaomu_runtime::session::{SessionError, SessionOutcome};

use super::{DocumentView, visual_navigation::NavStep};
use crate::editor_commands::{EditorCommandContext, NodeNavigation, NodeNavigationDirection};

#[cfg(test)]
#[path = "node_selection_tests.rs"]
mod tests;

impl DocumentView {
    /// Selects a complete supported block and explicitly focuses this view.
    ///
    /// Runtime validates the target before any frontend state changes. Errors
    /// preserve selection, focus and history. `Ok(None)` means active native
    /// composition blocked this command; no preedit waiting protocol is added.
    /// Both a changed selection and `NoChange` restore native input focus, so
    /// an explicit repeated command can reclaim focus from another pane.
    /// Passive rendering of a background view never calls this method.
    pub fn select_node(
        &mut self,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<SessionOutcome>, SessionError> {
        if self.focused_child_composing(window, cx) {
            return Ok(None);
        }
        let outcome = self.session.borrow_mut().set_node_selection(node)?;
        self.desired_x = None;
        self.sync_children(cx);
        self.route_focus(window, cx);
        self.request_focus_scroll(cx);
        cx.notify();
        Ok(Some(outcome))
    }

    /// Decorates exactly the selected block's complete rendered subtree.
    /// All block kinds enter here, including tables and atomic renderers.
    pub(super) fn render_node_selection(
        &self,
        node: NodeId,
        block: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.session.borrow().selection().as_node_selection() != Some(node) {
            return block;
        }
        let mut selected = div()
            .debug_selector(move || format!("node-selection-{node:?}"))
            .relative()
            .w_full()
            .min_w_0()
            .min_h(px(28.0))
            .border_1()
            .border_color(gpui::rgba(0x2b6cb8ff))
            .bg(gpui::rgba(0x3377cc22))
            .child(block);
        if let Some((anchor, input)) = &self.range_input
            && *anchor == node
        {
            let scrolling_input = input.clone();
            let scroll_handle = self.scroll_handle.clone();
            let session = self.session.clone();
            selected = selected.child(
                canvas(
                    move |bounds, window, cx| {
                        let request = {
                            let input = scrolling_input.read(cx);
                            let focus = input.focus_handle(cx);
                            // Passive/background paints never scroll. During
                            // preedit the native caret owns its own visibility.
                            if focus.is_focused(window) && !input.is_composing() {
                                input
                                    .take_selected_node_scroll_offset(&bounds)
                                    .map(|offset| (offset, focus))
                            } else {
                                None
                            }
                        };
                        if let Some((offset, focus)) = request {
                            let previous = scroll_handle.offset();
                            let viewport = scroll_handle.bounds();
                            let maximum = scroll_handle.max_offset();
                            let revision = session.borrow().document().revision();
                            // Apply after the current draw/effect cycle so all
                            // children paint with the same scroll coordinates.
                            // Unlike a display-link callback, this is also
                            // delivered by stock GPUI's virtual test window.
                            window.defer(cx, move |window, cx| {
                                if !focus.is_focused(window)
                                    || session.borrow().selection().as_node_selection()
                                        != Some(node)
                                    || scroll_handle.offset() != previous
                                {
                                    return;
                                }
                                if scroll_handle.bounds() != viewport
                                    || scroll_handle.max_offset() != maximum
                                    || session.borrow().document().revision() != revision
                                {
                                    // Another edit or viewport resize made the
                                    // captured geometry stale. Re-measure; do
                                    // not apply the old coordinates.
                                    scrolling_input.read(cx).request_caret_scroll();
                                    window.refresh();
                                    return;
                                }
                                scroll_handle.set_offset(offset);
                                window.refresh();
                            });
                        }
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
            selected = selected.child(
                div()
                    .debug_selector(move || format!("node-selection-input-{node:?}"))
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .when(input.read(cx).is_composing(), |proxy| {
                        proxy.bg(gpui::white())
                    })
                    .child(input.clone()),
            );
        }
        selected.into_any_element()
    }

    /// Consumes every explicit node navigation gesture, even without a router.
    pub(super) fn navigate_node_selection(
        &mut self,
        step: &NavStep,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .session
            .borrow()
            .selection()
            .as_node_selection()
            .is_none()
        {
            return false;
        }
        let Some(router) = &self.command_router else {
            return true;
        };
        let direction = match step {
            NavStep::Left => NodeNavigationDirection::Left,
            NavStep::Right => NodeNavigationDirection::Right,
            NavStep::Up => NodeNavigationDirection::Up,
            NavStep::Down => NodeNavigationDirection::Down,
            NavStep::LineStart => NodeNavigationDirection::Home,
            NavStep::LineEnd => NodeNavigationDirection::End,
        };
        let target = {
            let session = self.session.borrow();
            router.route_node_navigation(
                EditorCommandContext::from_session(&session),
                NodeNavigation { direction, extend },
            )
        };
        match target {
            Ok(Some(selection)) => {
                let outcome = self.session.borrow_mut().set_document_selection(selection);
                match outcome {
                    Ok(_) => {
                        self.desired_x = None;
                        self.sync_children(cx);
                        self.route_focus(window, cx);
                        self.request_focus_scroll(cx);
                        cx.notify();
                    }
                    Err(error) => eprintln!("xiaomu: node navigation selection rejected: {error}"),
                }
            }
            Ok(None) => {}
            Err(error) => eprintln!("xiaomu: host node navigation rejected: {error}"),
        }
        true
    }
}
