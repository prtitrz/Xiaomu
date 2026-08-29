//! IME composition state machine.
//!
//! Marked text (preedit) is a frontend transient: it never enters the
//! canonical document and never advances its revision. While composing,
//! every [`crate::block_view`] input-handler query is answered against a
//! *virtual projection* of the node text: canonical prefix + preedit +
//! canonical suffix.
//!
//! Platform callback mapping for the pinned GPUI 0.2.2 (verified against the
//! crates.io sources, `platform/mac/window.rs` and
//! `platform/windows/events.rs`):
//!
//! ```text
//! macOS   setMarkedText(text)  → begin_or_update; empty text = cancel
//! macOS   insertText           → commit (non-empty) / cancel (empty)
//! macOS   unmarkText           → cancel if still composing, else no-op
//! Windows GCS_COMPSTR          → begin_or_update (caret range inside preedit)
//! Windows GCS_COMPSTR ""       → cancel (composition ended without a result,
//!                                e.g. Esc on Microsoft Pinyin)
//! Windows GCS_RESULTSTR        → commit (non-empty)
//! Windows WM_IME_COMPOSITION lparam == 0
//!                              → cancel disguised as replace_text_in_range("")
//! Windows WM_CHAR (plain ASCII)→ replace_text_in_range, only valid while idle
//! ```
//!
//! A commit carrying empty text is indistinguishable from the Windows cancel
//! path, so it is treated as a cancel: real IME results are never empty, and
//! an empty insertion over the base range would wrongly delete a selection.

use super::utf16;

/// Transient IME composition state for one inline node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompositionState {
    /// Half-open canonical byte range the committed text will replace.
    base_range: core::ops::Range<usize>,
    /// Current marked text.
    preedit: String,
    /// Selected range inside the preedit, in UTF-16 code units.
    preedit_selected_utf16: core::ops::Range<usize>,
}

/// How a marked-text (preedit) callback resolves against an active
/// composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreeditUpdate {
    /// Keep composing with the new preedit.
    Continue,
    /// The composition ended without committing (empty marked text); the
    /// frontend drops its transient projection.
    Cancelled,
}

/// Resolves a `replace_and_mark_text_in_range` payload received while a
/// composition is active.
///
/// An empty marked string ends the composition: Microsoft Pinyin reports an
/// Esc cancellation exactly as `GCS_COMPSTR` with an empty string, and
/// macOS `setMarkedText("")` likewise removes the marking. Keeping an empty
/// preedit alive here would strand the adapter in composing state, blocking
/// all keyboard editing until the next click.
pub(crate) fn resolve_preedit_update(new_text: &str) -> PreeditUpdate {
    if new_text.is_empty() {
        PreeditUpdate::Cancelled
    } else {
        PreeditUpdate::Continue
    }
}

/// Returns the UTF-16 code-unit length of `text`.
fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

impl CompositionState {
    /// Begins composition and installs the first preedit over `base_range`
    /// (canonical byte offsets).
    ///
    /// `preedit_selected_utf16` is the platform-provided selection inside
    /// the preedit; `None` collapses to the end of the preedit.
    pub(crate) fn begin(
        base_range: core::ops::Range<usize>,
        preedit: &str,
        preedit_selected_utf16: Option<core::ops::Range<usize>>,
    ) -> Self {
        Self::apply_update(
            Self {
                base_range,
                preedit: String::new(),
                preedit_selected_utf16: 0..0,
            },
            preedit,
            preedit_selected_utf16,
        )
    }

    /// Replaces the preedit text, keeping the base untouched.
    ///
    /// Callers resolve empty payloads through [`resolve_preedit_update`] and
    /// never call `update` with an empty string.
    pub(crate) fn update(
        &self,
        preedit: &str,
        preedit_selected_utf16: Option<core::ops::Range<usize>>,
    ) -> Self {
        Self::apply_update(self.clone(), preedit, preedit_selected_utf16)
    }

    fn apply_update(
        mut state: Self,
        preedit: &str,
        preedit_selected_utf16: Option<core::ops::Range<usize>>,
    ) -> Self {
        state.preedit = preedit.to_owned();
        state.preedit_selected_utf16 =
            preedit_selected_utf16.unwrap_or_else(|| 0..utf16_len(preedit));
        state
    }

    /// Returns the canonical half-open byte range the commit replaces.
    #[must_use]
    pub(crate) fn base_range(&self) -> core::ops::Range<usize> {
        self.base_range.clone()
    }

    /// Returns the current marked text.
    #[must_use]
    pub(crate) fn preedit(&self) -> &str {
        &self.preedit
    }

    /// Builds the virtual projection: canonical text with the preedit
    /// spliced over the base range.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn project(&self, canonical: &str) -> String {
        let mut projected = String::with_capacity(
            canonical.len() - (self.base_range.end - self.base_range.start) + self.preedit.len(),
        );
        projected.push_str(&canonical[..self.base_range.start]);
        projected.push_str(&self.preedit);
        projected.push_str(&canonical[self.base_range.end.min(canonical.len())..]);
        projected
    }

    /// Returns the marked range in virtual-text UTF-16 code units.
    #[must_use]
    pub(crate) fn marked_range_virtual_utf16(&self, canonical: &str) -> core::ops::Range<usize> {
        let start_units = utf16::utf16_offset(canonical, self.base_range.start);
        start_units..start_units + utf16_len(&self.preedit)
    }

    /// Returns the preedit selection mapped into virtual-text UTF-16 units.
    #[must_use]
    pub(crate) fn selected_range_virtual_utf16(&self, canonical: &str) -> core::ops::Range<usize> {
        let marked = self.marked_range_virtual_utf16(canonical);
        marked.start + self.preedit_selected_utf16.start
            ..marked.start + self.preedit_selected_utf16.end
    }

    /// Returns the virtual-text UTF-8 byte offset of the composition caret.
    #[must_use]
    pub(crate) fn caret_virtual_byte(&self) -> usize {
        self.base_range.start + utf16::utf8_offset(&self.preedit, self.preedit_selected_utf16.end)
    }
}

/// Outcome resolved by the view when the platform ends a composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CompositionEnd {
    /// The platform committed `text`; the view inserts it over the base
    /// range as one undo unit.
    Committed(String),
    /// The composition ended without text; the frontend drops the preedit.
    Cancelled,
}

/// Resolves a platform commit signal into [`CompositionEnd`].
///
/// An empty committed string is treated as a cancellation because the
/// Windows cancel path surfaces exactly that way through GPUI.
pub(crate) fn resolve_commit_signal(text: &str) -> CompositionEnd {
    if text.is_empty() {
        CompositionEnd::Cancelled
    } else {
        CompositionEnd::Committed(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Canonical fixture: "你好world" (你=0..3 好=3..6 w=6..7 ... d=10..11).
    const CANONICAL: &str = "你好world";

    fn state_with_base(start: usize, end: usize) -> CompositionState {
        CompositionState::begin(start..end, "", None)
    }

    #[test]
    fn begin_captures_base_and_installs_preedit() {
        let state = CompositionState::begin(
            6..6,
            "nihao",
            Some(2..3), // caret inside pinyin, like Microsoft Pinyin
        );

        assert_eq!(state.base_range(), 6..6);
        assert_eq!(state.project(CANONICAL), "你好nihaoworld");
        assert_eq!(state.marked_range_virtual_utf16(CANONICAL), 2..7);
        assert_eq!(state.selected_range_virtual_utf16(CANONICAL), 4..5);
        assert_eq!(state.caret_virtual_byte(), 6 + 3); // after "nih"
    }

    #[test]
    fn update_replaces_preedit_and_keeps_base() {
        let state = state_with_base(6, 6).update("ni", None);
        assert_eq!(state.project(CANONICAL), "你好niworld");

        // Continuous composition: candidates narrow the pinyin.
        let state = state.update("nihao", None);
        assert_eq!(state.project(CANONICAL), "你好nihaoworld");
        assert_eq!(state.base_range(), 6..6);
    }

    #[test]
    fn empty_preedit_payloads_end_the_composition() {
        // Continuous pinyin stays alive...
        assert_eq!(resolve_preedit_update("ni'hao"), PreeditUpdate::Continue);
        // ...while an empty marked string is a cancellation on every pinned
        // platform (Esc on Microsoft Pinyin, setMarkedText("") on macOS).
        assert_eq!(resolve_preedit_update(""), PreeditUpdate::Cancelled);
    }

    #[test]
    fn projection_over_a_non_collapsed_base_replaces_the_range() {
        // Base covers "你好" [0, 6): preedit splices over the whole run.
        let state = CompositionState::begin(0..6, "ABC", None);
        assert_eq!(state.project(CANONICAL), "ABCworld");
        assert_eq!(state.marked_range_virtual_utf16(CANONICAL), 0..3);
    }

    #[test]
    fn commit_and_cancel_signals_resolve() {
        assert_eq!(
            resolve_commit_signal("你好"),
            CompositionEnd::Committed("你好".to_owned())
        );
        // Windows cancel arrives as an empty plain replacement.
        assert_eq!(resolve_commit_signal(""), CompositionEnd::Cancelled);
    }

    #[test]
    fn emoji_preedit_offsets_stay_on_scalar_boundaries() {
        let state = state_with_base(11, 11).update("👍中", None);

        // Prefix "你好world" is 7 UTF-16 units; 👍 = 4 bytes / 2 units, 中 = 3 bytes / 1 unit.
        assert_eq!(state.marked_range_virtual_utf16(CANONICAL), 7..10);
        assert_eq!(state.caret_virtual_byte(), 11 + 7);
        assert_eq!(state.project(CANONICAL), "你好world👍中");
    }

    #[test]
    fn default_selection_collapses_at_preedit_end() {
        let state = state_with_base(0, 0).update("拼音", None);
        let marked = state.marked_range_virtual_utf16(CANONICAL);
        assert_eq!(state.selected_range_virtual_utf16(CANONICAL), marked);
    }
}
