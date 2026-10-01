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

use super::ParagraphView;

impl ParagraphView {
    /// Ends the composition without committing canonical content.
    ///
    /// The Runtime selection never moved while preedit was active, so cancel
    /// must not synthesize `PlaceCaret` intents. This also preserves pending
    /// StoredMarks across an IME cancellation.
    pub(crate) fn cancel_composition(&mut self, cx: &mut Context<Self>) {
        let rejected = std::mem::take(&mut self.rejected_composition);
        if self.composition.take().is_none() && !rejected {
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
        if self.rejected_composition {
            if new_text.is_empty() {
                self.cancel_composition(cx);
            }
            return;
        }
        if self.composition.is_none() {
            // An empty payload cannot start a composition; ignore it.
            if new_text.is_empty() {
                return;
            }
            let Some((start, end)) = self.input_range_points(range_utf16) else {
                return;
            };
            let Some(inline) = self.inline() else {
                return;
            };
            // A native text range cannot replace a selected chip. Preserve
            // the existing fail-closed contract rather than hiding its bytes.
            let start_key = (start.text_offset(), start.atom_index());
            let end_key = (end.text_offset(), end.atom_index());
            let mut previous = None;
            let mut ordinal = 0;
            let crosses_atom = inline.atoms().iter().any(|atom| {
                ordinal = if previous == Some(atom.text_offset()) {
                    ordinal + 1
                } else {
                    0
                };
                previous = Some(atom.text_offset());
                let key = (atom.text_offset(), ordinal);
                key >= start_key && key < end_key
            });
            if crosses_atom {
                self.rejected_composition = true;
                eprintln!("xiaomu: IME range spans an inline atom; composition rejected");
                return;
            }

            self.composition = Some(
                CompositionState::begin(
                    start.text_offset().as_usize()..end.text_offset().as_usize(),
                    new_text,
                    new_selected_range,
                )
                .at_atom_gap(start.atom_index()),
            );
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
