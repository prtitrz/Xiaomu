//! Scroll-to-caret support for one inline-bearing block.
//!
//! The owning `DocumentView` provides one shared GPUI `ScrollHandle`. A
//! focused block computes caret bounds from its wrapped layout and requests
//! the smallest vertical viewport adjustment needed to keep the focus visible.

use gpui::{Bounds, Pixels, Point, Window};

use super::ParagraphView;

impl ParagraphView {
    /// Keeps a whole selected block visible without inventing a text caret.
    /// Oversized containers reveal their leading viewport-sized portion.
    pub(crate) fn take_selected_node_scroll_offset(
        &self,
        bounds: &Bounds<Pixels>,
    ) -> Option<Point<Pixels>> {
        let scroll = self.scroll_handle.as_ref()?;
        let mut visible = *bounds;
        visible.size.height = visible.size.height.min(scroll.bounds().size.height);
        self.take_keep_visible_offset(&visible)
    }

    /// Marks the next focused-caret prepaint as needing a keep-visible check.
    ///
    /// Passive viewport scrolling never sets this flag. This distinction lets
    /// users deliberately scroll away from the caret without the editor
    /// immediately snapping the viewport back on the next paint.
    pub(crate) fn request_caret_scroll(&self) {
        self.scroll_caret_pending.set(true);
    }

    /// Requests the minimum vertical scroll needed to keep `caret` visible.
    ///
    /// `caret` is expressed in window coordinates. Scroll changes are applied
    /// after the current frame so every child of the tracked viewport observes
    /// one consistent scroll offset during prepaint and paint.
    pub(crate) fn keep_caret_visible(&self, caret: &Bounds<Pixels>, window: &mut Window) {
        let Some(offset) = self.take_keep_visible_offset(caret) else {
            return;
        };
        let scroll_handle = self
            .scroll_handle
            .as_ref()
            .expect("validated scroll handle")
            .clone();
        window.on_next_frame(move |_, _| scroll_handle.set_offset(offset));
    }

    fn take_keep_visible_offset(&self, caret: &Bounds<Pixels>) -> Option<Point<Pixels>> {
        if !self.scroll_caret_pending.get() {
            return None;
        }
        let scroll_handle = self.scroll_handle.as_ref()?;
        let viewport = scroll_handle.bounds();
        if viewport.size.height <= Pixels::ZERO {
            return None;
        }

        // A valid viewport has observed this request, even if the caret is
        // already visible and no offset change is required.
        self.scroll_caret_pending.set(false);

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

        (offset.y != original_y).then_some(offset)
    }
}
