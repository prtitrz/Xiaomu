//! IME composition lifecycle of the single-paragraph view.
//!
//! Split out of [`super::ParagraphView`]'s main module to keep file sizes
//! within the source-size guardrail. The composition state machine itself
//! lives in [`crate::input::composition`]; this module wires it to the
//! view: begin/update remain frontend-transient, commit becomes one Runtime
//! composition intent, and cancel simply drops the transient projection.

use gpui::Context;
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_runtime::session::EditIntent;

use crate::input::composition::{CompositionState, PreeditUpdate, resolve_preedit_update};
use crate::input::utf16;

use super::ParagraphView;

impl ParagraphView {
    /// Ends the composition without committing canonical content.
    ///
    /// The Runtime selection never moved while preedit was active, so cancel
    /// must not synthesize `PlaceCaret` intents. This also preserves pending
    /// StoredMarks across an IME cancellation.
    pub(crate) fn cancel_composition(&mut self, cx: &mut Context<Self>) {
        if self.composition.take().is_none() {
            return;
        }
        self.request_caret_scroll();
        cx.notify();
    }

    /// Commits `text` over the composition's canonical base range as exactly
    /// one Runtime intent and one undo unit.
    pub(crate) fn commit_composition(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(state) = self.composition.take() else {
            return;
        };

        let range = state.base_range();
        let Ok(start) = self
            .inline()
            .map(|inline| inline.offset_at(range.start))
            .unwrap_or_else(|| Ok(TextOffset::ZERO))
        else {
            return;
        };
        let Ok(end) = self
            .inline()
            .map(|inline| inline.offset_at(range.end))
            .unwrap_or_else(|| Ok(TextOffset::ZERO))
        else {
            return;
        };
        let Ok(range) = TextRange::new(start, end) else {
            return;
        };

        self.apply_intent(
            EditIntent::CommitComposition {
                range,
                text: text.to_owned(),
            },
            cx,
        );
    }

    /// Begins or updates the IME composition with a new preedit string.
    pub(crate) fn mark_text(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        new_selected_range: Option<std::ops::Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let canonical = self.canonical_text();

        if self.composition.is_none() {
            // An empty payload cannot start a composition; ignore it.
            if new_text.is_empty() {
                return;
            }
            let (start, end) = match range_utf16 {
                Some(range) => (
                    utf16::utf8_offset(&canonical, range.start),
                    utf16::utf8_offset(&canonical, range.end),
                ),
                None => match self.ordered_range() {
                    Some(range) => (range.start().as_usize(), range.end().as_usize()),
                    None => return,
                },
            };

            let Some(inline) = self.inline() else {
                return;
            };
            let (Ok(start), Ok(end)) = (inline.offset_at(start), inline.offset_at(end)) else {
                return;
            };

            self.composition = Some(CompositionState::begin(
                start.as_usize()..end.as_usize(),
                new_text,
                new_selected_range,
            ));
        } else {
            match resolve_preedit_update(new_text) {
                PreeditUpdate::Continue => {
                    if let Some(state) = self.composition.as_mut() {
                        *state = state.update(new_text, new_selected_range);
                    }
                }
                PreeditUpdate::Cancelled => {
                    // Windows reports cancellations (e.g. Esc on Microsoft
                    // Pinyin) as an empty GCS_COMPSTR through the marked-text
                    // path; macOS sends an empty setMarkedText / unmarkText.
                    // All of them must clear the composition state here,
                    // otherwise every keyboard edit stays blocked by the
                    // composing guard until the next mouse click.
                    self.cancel_composition(cx);
                    return;
                }
            }
        }

        // The preedit is a view transient: without an explicit repaint the
        // marked text would stay invisible while the IME session continues.
        // It is also an explicit caret movement event for viewport purposes;
        // passive scrolling alone never sets this request.
        self.request_caret_scroll();
        cx.notify();
    }

    /// Cancels the composition if one is active; used on focus loss.
    pub(crate) fn cancel_if_composing(&mut self, cx: &mut Context<Self>) {
        if self.is_composing() {
            self.cancel_composition(cx);
        }
    }
}
