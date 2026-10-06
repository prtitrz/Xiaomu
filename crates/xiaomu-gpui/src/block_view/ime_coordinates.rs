//! Notify the platform after publishing changed, current-frame caret geometry.
//!
//! Stock GPUI queues its own next-frame selected-bounds query. In particular,
//! X11 may ignore an explicit position update during composition; this seam
//! does not repair synchronous preedit queries against an older painted layout
//! or claim native candidate-placement parity.

use gpui::{Bounds, Context, EntityInputHandler, Pixels, Window};

use super::ParagraphView;

#[cfg(test)]
#[path = "ime_coordinates_tests.rs"]
mod tests;

#[derive(Default)]
pub(super) struct ImeCoordinates {
    bounds: Option<Bounds<Pixels>>,
    #[cfg(test)]
    notifications: usize,
}

impl ImeCoordinates {
    pub(super) fn clear(&mut self) {
        self.bounds = None;
    }

    fn update(&mut self, bounds: Option<Bounds<Pixels>>) -> bool {
        let changed = self.bounds != bounds;
        self.bounds = bounds;
        let notify = changed && bounds.is_some();
        #[cfg(test)]
        if notify {
            self.notifications += 1;
        }
        notify
    }
}

impl ParagraphView {
    /// Called only after this paint publishes its layout and absolute bounds.
    /// Returning the decision lets the caller leave the entity borrow before
    /// invoking the platform invalidation API. Stable geometry never requests
    /// another frame/notification and hidden/unfocused views clear their stamp.
    pub(super) fn ime_coordinates_changed(
        &mut self,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let bounds = if self.focus_handle.is_focused(window) && window.is_window_active() {
            self.selected_text_range(true, window, cx)
                .and_then(|selection| {
                    // Match stock PlatformInputHandler::selected_bounds: only
                    // the active selection head anchors the candidate window.
                    let head = if selection.reversed {
                        selection.range.start
                    } else {
                        selection.range.end
                    };
                    self.bounds_for_range(head..head, element_bounds, window, cx)
                })
        } else {
            None
        };
        self.ime_coordinates.update(bounds)
    }
}
