//! Scroll-to-caret support for one inline-bearing block.
//!
//! The owning `DocumentView` provides one shared GPUI `ScrollHandle`. A
//! focused block computes caret bounds from its wrapped layout and requests
//! the smallest vertical viewport adjustment needed to keep the focus visible.

use gpui::{Bounds, Pixels, Window};

use super::ParagraphView;

impl ParagraphView {
    /// Requests the minimum vertical scroll needed to keep `caret` visible.
    ///
    /// `caret` is expressed in window coordinates. Scroll changes are applied
    /// after the current frame so every child of the tracked viewport observes
    /// one consistent scroll offset during prepaint and paint.
    pub(crate) fn keep_caret_visible(&self, caret: &Bounds<Pixels>, window: &mut Window) {
        let Some(scroll_handle) = self.scroll_handle.as_ref() else {
            return;
        };
        let viewport = scroll_handle.bounds();
        if viewport.size.height <= Pixels::ZERO {
            return;
        }

        let mut offset = scroll_handle.offset();
        let original_y = offset.y;

        if caret.top() < viewport.top() {
            offset.y += viewport.top() - caret.top();
        } else if caret.bottom() > viewport.bottom() {
            offset.y -= caret.bottom() - viewport.bottom();
        }

        let minimum_y = Pixels::ZERO - scroll_handle.max_offset().height;
        if offset.y > Pixels::ZERO {
            offset.y = Pixels::ZERO;
        } else if offset.y < minimum_y {
            offset.y = minimum_y;
        }

        if offset.y == original_y {
            return;
        }

        let scroll_handle = scroll_handle.clone();
        window.on_next_frame(move |_, _| scroll_handle.set_offset(offset));
    }
}
