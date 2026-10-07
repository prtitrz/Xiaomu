//! Drop geometry and caret-scroll requests superseded by a passive plan.

use super::ParagraphView;

impl ParagraphView {
    pub(crate) fn passive_scroll_epoch(&self) -> u64 {
        self.passive_scroll_epoch.get()
    }

    pub(crate) fn invalidate_passive_layout(&mut self) {
        self.last_layout = None;
        self.last_bounds = None;
        self.last_caret = None;
        self.cache_key = None;
        self.ime_coordinates.clear();
        // New paragraph/range surfaces request scrolling by default. A passive
        // publication must cancel that request as well as an old pending one.
        self.scroll_caret_pending.set(false);
        self.passive_scroll_epoch
            .set(self.passive_scroll_epoch.get().wrapping_add(1));
    }
}
