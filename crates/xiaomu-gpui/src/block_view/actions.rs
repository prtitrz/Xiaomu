//! Clipboard and mark-toggle actions of the single-paragraph view.
//!
//! Split out of [`super::ParagraphView`]'s main module to keep file sizes
//! within the source-size guardrail. All editing still flows through
//! runtime intents; this module only translates GPUI actions into them and
//! moves plain text through the runtime clipboard seam.

use gpui::{Context, Window};

use xiaomu_core::document::Mark;
use xiaomu_runtime::clipboard::{TextClipboard, normalize_paste_text};
use xiaomu_runtime::session::EditIntent;

use crate::input::platform_clipboard::PlatformClipboard;

use super::{
    ClipboardCopy, ClipboardCut, ClipboardPaste, ParagraphView, ToggleBold, ToggleCode,
    ToggleItalic, ToggleStrike, ToggleUnderline,
};

impl ParagraphView {
    fn clipboard_text(&self, cx: &Context<Self>) -> Option<String> {
        PlatformClipboard::new(cx).read_text()
    }

    pub(super) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if self.is_composing() {
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        if let Some(text) = self.session.selected_text() {
            PlatformClipboard::new(cx).write_text(text);
        }
    }

    pub(super) fn copy(&mut self, _: &ClipboardCopy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    pub(super) fn cut(&mut self, _: &ClipboardCut, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_composing() {
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        if let Some(text) = self.session.selected_text() {
            PlatformClipboard::new(cx).write_text(text);
            // A non-collapsed selection deletes as a whole; one undo unit.
            self.apply_intent_when_idle(EditIntent::Delete, cx);
        }
    }

    pub(super) fn paste(&mut self, _: &ClipboardPaste, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_composing() {
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        let Some(text) = self.clipboard_text(cx) else {
            return;
        };
        // Line breaks cannot exist inside a paragraph's inline text; pasted
        // breaks become spaces. Empty text must not clear the selection.
        let text = normalize_paste_text(&text);
        if !text.is_empty() {
            self.apply_intent_when_idle(
                EditIntent::InsertText {
                    text: text.to_owned(),
                },
                cx,
            );
        }
    }

    pub(super) fn toggle_mark(&mut self, mark: Mark, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(EditIntent::ToggleMark { mark }, cx);
    }

    pub(super) fn toggle_bold(&mut self, _: &ToggleBold, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Bold, cx);
    }

    pub(super) fn toggle_italic(
        &mut self,
        _: &ToggleItalic,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_mark(Mark::Italic, cx);
    }

    pub(super) fn toggle_code(&mut self, _: &ToggleCode, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Code, cx);
    }

    pub(super) fn toggle_underline(
        &mut self,
        _: &ToggleUnderline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_mark(Mark::Underline, cx);
    }

    pub(super) fn toggle_strike(
        &mut self,
        _: &ToggleStrike,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_mark(Mark::Strike, cx);
    }
}
