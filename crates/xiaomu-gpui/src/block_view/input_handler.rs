//! `EntityInputHandler` implementation for [`ParagraphView`].
//!
//! Every query answers against the virtual projection while an IME
//! composition is active (see [`crate::input::composition`]); platform
//! UTF-16 ranges are converted at this boundary only.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{Bounds, EntityInputHandler, Pixels, Point, UTF16Selection, Window, point, size};

use xiaomu_runtime::session::EditIntent;

use crate::input::composition::{CompositionEnd, resolve_commit_signal};
use crate::input::utf16;

use super::ParagraphView;

impl EntityInputHandler for ParagraphView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        if self.input_is_hidden_by_table() {
            return None;
        }
        let text = self.display_content().0;
        let start = utf16::utf8_offset(&text, range_utf16.start);
        let end = utf16::utf8_offset(&text, range_utf16.end);
        adjusted_range.replace(utf16::utf16_offset(&text, start)..utf16::utf16_offset(&text, end));
        Some(text.get(start..end)?.to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.input_is_hidden_by_table() {
            return None;
        }
        let text = self.display_content().0;
        if let Some(state) = self.composition.as_ref() {
            let range = state.selected_range_virtual_utf16(&self.canonical_text());
            return Some(UTF16Selection {
                range,
                reversed: false,
            });
        }

        if self.is_range_input() {
            return Some(UTF16Selection {
                range: 0..0,
                reversed: false,
            });
        }
        let (anchor, focus) = self.session.borrow().selection().as_same_node_inline()?;
        if anchor.node_id() != self.node() {
            return None;
        }
        let a = (anchor.text_offset().as_usize(), anchor.atom_index());
        let f = (focus.text_offset().as_usize(), focus.atom_index());
        let (start, end) = (a.0.min(f.0), a.0.max(f.0));
        Some(UTF16Selection {
            range: utf16::utf16_offset(&text, start)..utf16::utf16_offset(&text, end),
            // The platform sees the focus (cursor) as the selection head.
            reversed: f < a,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        if self.input_is_hidden_by_table() {
            return None;
        }
        self.composition
            .as_ref()
            .map(|state| state.marked_range_virtual_utf16(&self.canonical_text()))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.input_is_hidden_by_table() {
            self.cancel_if_composing(cx);
            return;
        }
        // Stock GPUI's Linux mouse path unmarks the old input handler before
        // dispatching MouseDown. Unlike Zed's in-buffer preedit, our overlay
        // is not canonical yet: removing its marker must retain the text the
        // application received, not silently discard it. Explicit empty
        // callbacks have already cancelled it; result callbacks already
        // committed it. Neither can be committed a second time here.
        #[cfg(target_os = "linux")]
        if !self.rejected_composition
            && let Some(text) = self
                .composition
                .as_ref()
                .map(|state| state.preedit())
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        {
            self.commit_composition(&text, cx);
            return;
        }
        // Preserve existing handling elsewhere pending platform-specific
        // native evidence. Focus-loss cancellation is a separate path.
        self.cancel_if_composing(cx);
    }

    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input_is_hidden_by_table() {
            self.cancel_if_composing(cx);
            return;
        }
        if self.rejected_composition {
            self.cancel_composition(cx);
            return;
        }
        if self.composition.is_some() {
            // macOS commits through here (insertText); Windows ends a
            // composition through here as well — including cancellations,
            // which arrive as an empty replacement.
            match resolve_commit_signal(text) {
                CompositionEnd::Committed(committed) => self.commit_composition(&committed, cx),
                CompositionEnd::Cancelled => self.cancel_composition(cx),
            }
            return;
        }

        let selection =
            if let Some(range_utf16) = replacement_range.filter(|_| !self.is_range_input()) {
                // Preserve a platform echo's seam ordinal instead of converting
                // it through the legacy text-only PlaceCaret intent.
                let Some((start, end)) = self.input_range_points(Some(range_utf16)) else {
                    return;
                };
                Some(xiaomu_runtime::session::DocumentSelection::new(start, end))
            } else {
                None
            };
        self.apply_intent_with_selection(
            EditIntent::InsertText {
                text: text.to_owned(),
            },
            selection,
            cx,
        );
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Begin or continue composition; the preedit never touches the
        // canonical document.
        self.mark_text(range_utf16, new_text, new_selected_range, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Stock GPUI may query the previously installed handler before the
        // next draw after a focus/window switch. Candidate geometry belongs
        // only to the current native owner, including deferred refreshes.
        if !self.focus_handle.is_focused(window) || self.input_is_hidden_by_table() {
            return None;
        }
        // Linux activation is synchronous before native IME queries. macOS
        // and Windows can request geometry before their async active callback;
        // preserve that existing geometry path rather than assuming its order.
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        if !window.is_window_active() {
            return None;
        }
        let layout = self.last_layout.as_ref()?;
        let text = self.display_content().0;
        let start = utf16::utf8_offset(&text, range_utf16.start);
        let end = utf16::utf8_offset(&text, range_utf16.end);
        let start = self.input_byte_to_layout(start)?;
        let end = self.input_byte_to_layout(end)?;
        let caret = self
            .composing_caret_byte()
            .map(|byte| (byte, xiaomu_core::selection::CursorAffinity::Before))
            .or_else(|| self.display_focus_caret());
        let start_position = if layout.has_alignment()
            && start == end
            && caret.is_some_and(|(byte, _)| byte == start)
        {
            layout.position_for_caret(start, caret?.1)?
        } else {
            layout.position_for_index(start)?
        };
        let end_position = if start == end {
            start_position
        } else {
            layout.position_for_index(end)?
        };

        let left = start_position.x.min(end_position.x);
        let right = start_position.x.max(end_position.x);
        let top = start_position.y.min(end_position.y);
        let bottom = start_position.y.max(end_position.y) + layout.line_height();
        let mut bounds = Bounds::new(
            point(left, top),
            size((right - left).max(Pixels::from(1.0)), bottom - top),
        );
        if layout.has_alignment() && start != end {
            // Endpoint x positions need not enclose wider intermediate rows.
            // Only include selected rows: an endpoint at a wrap or just after
            // LF can otherwise add an entirely unselected adjacent row.
            let mut rects = layout.selection_rects(start..end).into_iter();
            if let Some(first) = rects.next() {
                bounds = rects.fold(first, |bounds, rect| bounds.union(&rect));
            }
        }
        bounds.origin += element_bounds.origin;
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        point_in_window: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        if self.input_is_hidden_by_table() {
            return None;
        }
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let text = self.display_content().0;
        let raw = if point_in_window.y < bounds.top() {
            0
        } else if point_in_window.y > bounds.bottom() {
            self.layout_content().0.len()
        } else {
            layout.closest_index_for_position(point(
                point_in_window.x - bounds.left(),
                point_in_window.y - bounds.top(),
            ))
        };
        Some(utf16::utf16_offset(
            &text,
            self.layout_byte_to_input(raw)?.min(text.len()),
        ))
    }
}
