//! `EntityInputHandler` implementation for [`ParagraphView`].
//!
//! Every query answers against the virtual projection while an IME
//! composition is active (see [`crate::input::composition`]); platform
//! UTF-16 ranges are converted at this boundary only.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{Bounds, EntityInputHandler, Pixels, Point, UTF16Selection, Window};

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
        let text = self.display_content().0;
        if let Some(state) = self.composition.as_ref() {
            let range = state.selected_range_virtual_utf16(&self.canonical_text());
            return Some(UTF16Selection {
                range,
                reversed: false,
            });
        }

        let selection = self.session.selection();
        let anchor = selection.anchor().offset().as_usize();
        let focus = selection.focus().offset().as_usize();
        let (start, end) = (anchor.min(focus), anchor.max(focus));
        Some(UTF16Selection {
            range: utf16::utf16_offset(&text, start)..utf16::utf16_offset(&text, end),
            // The platform sees the focus (cursor) as the selection head.
            reversed: focus < anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.composition
            .as_ref()
            .map(|state| state.marked_range_virtual_utf16(&self.canonical_text()))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        // macOS sends unmarkText both after a commit (already idle: no-op)
        // and as a pure cancellation (still composing).
        self.cancel_if_composing(cx);
    }

    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

        if let Some(range_utf16) = replacement_range {
            // Select the explicit range with selection-only intents (no
            // history entries), then insert over it as one transaction.
            let full_text = self.canonical_text();
            let start = utf16::utf8_offset(&full_text, range_utf16.start);
            let end = utf16::utf8_offset(&full_text, range_utf16.end);
            let Some(inline) = self.inline() else {
                return;
            };
            let (Ok(start), Ok(end)) = (inline.offset_at(start), inline.offset_at(end)) else {
                return;
            };
            self.apply_intent(
                EditIntent::PlaceCaret {
                    offset: start,
                    extend_selection: false,
                },
                cx,
            );
            self.apply_intent(
                EditIntent::PlaceCaret {
                    offset: end,
                    extend_selection: true,
                },
                cx,
            );
        }
        self.apply_intent(
            EditIntent::InsertText {
                text: text.to_owned(),
            },
            cx,
        );
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        _: &mut Context<Self>,
    ) {
        // Begin or continue composition; the preedit never touches the
        // canonical document.
        self.mark_text(range_utf16, new_text, new_selected_range);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let text = self.display_content().0;
        let start = utf16::utf8_offset(&text, range_utf16.start);
        let end = utf16::utf8_offset(&text, range_utf16.end);
        Some(Bounds::from_corners(
            gpui::point(
                element_bounds.left() + layout.x_for_index(start),
                element_bounds.top(),
            ),
            gpui::point(
                element_bounds.left() + layout.x_for_index(end),
                element_bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let text = self.display_content().0;
        let raw = if point.y < bounds.top() {
            0
        } else if point.y > bounds.bottom() {
            text.len()
        } else {
            layout.closest_index_for_x(point.x - bounds.left())
        };
        Some(utf16::utf16_offset(&text, raw))
    }
}
