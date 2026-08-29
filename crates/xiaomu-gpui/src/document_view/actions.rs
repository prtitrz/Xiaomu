//! Action listeners of [`DocumentView`].
//!
//! Split out of `mod.rs` to stay under the source-size guardrail. All
//! handlers translate GPUI events into runtime intents or pure navigation
//! steps; the session remains the single mutation point.

use gpui::{App, Context, Entity, Focusable as _, Window, actions};

use xiaomu_core::document::{Mark, NodeKind};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::clipboard::{normalize_multiline_paste_text, normalize_paste_text};
use xiaomu_runtime::session::{DocumentPosition, EditIntent};

use crate::block_view::{
    Backspace, ClipboardCopy, ClipboardCut, ClipboardPaste, Delete, Down, End, Enter, Home, Left,
    Redo, Right, SaveDocument, SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft,
    SelectRight, SelectUp, ShiftTabIndent, TabIndent, ToggleBold, ToggleCode, ToggleItalic,
    ToggleStrike, ToggleUnderline, Undo, Up,
};

use crate::block_view::ParagraphView;
use crate::input::platform_clipboard::{PlatformClipboard, PlatformClipboardContent};

use super::{DocumentView, NavStep, markers, navigation};

actions!(
    xiaomu_gpui,
    [
        /// Insert one canonical LF without structurally splitting the block.
        HardBreak,
    ]
);

/// Soft tab inserted when Tab is pressed inside a plain block away from
/// offset 0 or anywhere inside a CodeBlock. A literal U+0009 has no glyph
/// advance in GPUI's shaper, so it looks like a no-op; four ASCII spaces
/// match the usual editor tab size without needing tab-stop layout.
const SOFT_TAB: &str = "    ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnterPlan {
    SplitBlock,
    InsertLineBreak,
}

fn enter_plan_for_kind(kind: &NodeKind) -> EnterPlan {
    if matches!(kind, NodeKind::CodeBlock) {
        EnterPlan::InsertLineBreak
    } else {
        EnterPlan::SplitBlock
    }
}

/// What Tab should do given the focused inline block. A non-CodeBlock inside
/// a list returns `None` so the caller keeps the list-item indent intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabPlan {
    /// Collapsed caret at an ordinary block start: gesture for "make list".
    ConvertToList,
    /// Insert a visible soft tab (four spaces).
    InsertSoftTab,
}

fn tab_plan_for_block(
    kind: &NodeKind,
    in_list: bool,
    collapsed: bool,
    offset: usize,
) -> Option<TabPlan> {
    if matches!(kind, NodeKind::CodeBlock) {
        return Some(TabPlan::InsertSoftTab);
    }
    if in_list {
        return None;
    }
    if collapsed && offset == 0 {
        Some(TabPlan::ConvertToList)
    } else {
        Some(TabPlan::InsertSoftTab)
    }
}

/// Whether an intent is a structural command whose no-op is worth surfacing.
#[cfg(debug_assertions)]
pub(crate) fn is_structural(intent: &EditIntent) -> bool {
    matches!(
        intent,
        EditIntent::SplitBlock
            | EditIntent::JoinWithPrevious
            | EditIntent::TurnInto { .. }
            | EditIntent::IndentListItem
            | EditIntent::OutdentListItem
    )
}

impl DocumentView {
    // ---- action listeners ----

    pub(crate) fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Left, false, window, cx);
    }

    pub(crate) fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Right, false, window, cx);
    }

    pub(crate) fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Up, false, window, cx);
    }

    pub(crate) fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Down, false, window, cx);
    }

    pub(crate) fn select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(NavStep::Left, true, window, cx);
    }

    pub(crate) fn select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(NavStep::Right, true, window, cx);
    }

    pub(crate) fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Up, true, window, cx);
    }

    pub(crate) fn select_down(
        &mut self,
        _: &SelectDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(NavStep::Down, true, window, cx);
    }

    pub(crate) fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::LineStart, false, window, cx);
    }

    pub(crate) fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::LineEnd, false, window, cx);
    }

    pub(crate) fn select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(NavStep::LineStart, true, window, cx);
    }

    pub(crate) fn select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(NavStep::LineEnd, true, window, cx);
    }

    pub(crate) fn select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self.session.borrow();
        let blocks = navigation::text_blocks(session.document());
        drop(session);
        let (Some(first), Some(last)) = (blocks.first(), blocks.last()) else {
            return;
        };
        let Some(start) = navigation::validated_offset(first, 0) else {
            return;
        };
        let Some(end) = navigation::validated_offset(last, last.text().len()) else {
            return;
        };
        let anchor = TextPoint::new(first.node, start, CursorAffinity::Before);
        let focus = TextPoint::new(last.node, end, CursorAffinity::Before);
        self.set_selection(anchor, focus, window, cx);
    }

    pub(crate) fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        let plan = self
            .focused_node_kind()
            .as_ref()
            .map(enter_plan_for_kind)
            .unwrap_or(EnterPlan::SplitBlock);
        match plan {
            EnterPlan::SplitBlock => self.apply_intent(EditIntent::SplitBlock, window, cx),
            EnterPlan::InsertLineBreak => {
                self.apply_intent(EditIntent::insert_line_break(), window, cx);
            }
        }
    }

    pub(crate) fn hard_break(
        &mut self,
        _: &HardBreak,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_intent(EditIntent::insert_line_break(), window, cx);
    }

    pub(crate) fn tab_indent(
        &mut self,
        _: &TabIndent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // CodeBlock owns Tab as text indentation even when nested inside a
        // list. Ordinary blocks keep the P2 list semantics: a list item
        // indents structurally, while a non-list block at offset 0 converts
        // to a list and other positions insert a visible soft tab.
        let plan = {
            let session = self.session.borrow();
            match session.selection().focus() {
                DocumentPosition::Text(point) => {
                    let kind = session.document().node(point.node_id()).map(|node| node.kind());
                    let in_list =
                        markers::list_context(session.document(), point.node_id()).is_some();
                    match (kind, session.text_selection()) {
                        (Some(kind), Some(selection)) => tab_plan_for_block(
                            kind,
                            in_list,
                            selection.is_collapsed(),
                            selection.focus().offset().as_usize(),
                        ),
                        _ => None,
                    }
                }
                DocumentPosition::Gap(_) => None,
            }
        };
        match plan {
            Some(TabPlan::ConvertToList) => {
                self.apply_intent(
                    EditIntent::TurnInto {
                        kind: NodeKind::BulletList,
                    },
                    window,
                    cx,
                );
                return;
            }
            Some(TabPlan::InsertSoftTab) => {
                self.apply_intent(
                    EditIntent::PasteText {
                        text: SOFT_TAB.to_owned(),
                    },
                    window,
                    cx,
                );
                return;
            }
            None => {}
        }
        self.apply_intent(EditIntent::IndentListItem, window, cx);
    }

    pub(crate) fn shift_tab_indent(
        &mut self,
        _: &ShiftTabIndent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Shift-Tab walks out the other way: nested items outdent one
        // level; a top-level item lifts back to a plain paragraph. CodeBlock
        // never participates in list structure through Tab/Shift-Tab in P3.5.
        if matches!(self.focused_node_kind(), Some(NodeKind::CodeBlock)) {
            return;
        }
        let lifts_out = {
            let session = self.session.borrow();
            match session.selection().focus() {
                DocumentPosition::Text(point) => matches!(
                    markers::list_context(session.document(), point.node_id()),
                    Some(context) if !context.nested
                ),
                DocumentPosition::Gap(_) => false,
            }
        };
        if lifts_out {
            self.apply_intent(
                EditIntent::TurnInto {
                    kind: NodeKind::Paragraph,
                },
                window,
                cx,
            );
            return;
        }
        self.apply_intent(EditIntent::OutdentListItem, window, cx);
    }

    pub(crate) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::Backspace, window, cx);
    }

    pub(crate) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::Delete, window, cx);
    }

    pub(crate) fn save_document(
        &mut self,
        _: &SaveDocument,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        let Some(adapter) = self.persistence.clone() else {
            return;
        };
        let document = self.session.borrow().document().clone();
        let outcome = adapter.borrow_mut().save(&document);
        match outcome {
            Ok(()) => {
                #[cfg(debug_assertions)]
                eprintln!("xiaomu: snapshot saved");
            }
            Err(error) => eprintln!("xiaomu: save failed: {error}"),
        }
    }

    pub(crate) fn undo_entry(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let outcome = self.session.borrow_mut().undo();
        self.after_history(outcome, cx);
    }

    pub(crate) fn redo_entry(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let outcome = self.session.borrow_mut().redo();
        self.after_history(outcome, cx);
    }

    fn after_history(
        &mut self,
        outcome: Result<
            xiaomu_runtime::session::SessionOutcome,
            xiaomu_runtime::session::SessionError,
        >,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            Ok(outcome) => {
                if outcome != xiaomu_runtime::session::SessionOutcome::NoChange {
                    self.epoch.set(self.epoch.get() + 1);
                }
                cx.notify();
            }
            Err(error) => eprintln!("xiaomu: history rejected the operation: {error}"),
        }
    }

    pub(crate) fn copy(&mut self, _: &ClipboardCopy, _: &mut Window, cx: &mut Context<Self>) {
        match self.session.borrow().clipboard_slice() {
            Ok(Some(slice)) => PlatformClipboard::new(&*cx).write_slice(&slice),
            Ok(None) => {}
            Err(error) => eprintln!("xiaomu: clipboard projection failed: {error}"),
        }
    }

    pub(crate) fn cut(&mut self, _: &ClipboardCut, window: &mut Window, cx: &mut Context<Self>) {
        let slice = match self.session.borrow().clipboard_slice() {
            Ok(Some(slice)) => slice,
            Ok(None) => return,
            Err(error) => {
                eprintln!("xiaomu: clipboard projection failed: {error}");
                return;
            }
        };
        PlatformClipboard::new(&*cx).write_slice(&slice);
        // Clipboard projection is read-only; Delete remains the one history
        // mutation for the whole cut command.
        self.apply_intent(EditIntent::Delete, window, cx);
    }

    pub(crate) fn paste(
        &mut self,
        _: &ClipboardPaste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(content) = PlatformClipboard::new(&*cx).read_content() else {
            return;
        };
        match content {
            PlatformClipboardContent::Structured(slice) => {
                self.apply_intent(EditIntent::PasteSlice { slice }, window, cx);
            }
            PlatformClipboardContent::Text(text) => {
                let preserve_breaks =
                    matches!(self.focused_node_kind(), Some(NodeKind::CodeBlock));
                let text = if preserve_breaks {
                    normalize_multiline_paste_text(&text)
                } else {
                    normalize_paste_text(&text)
                };
                if !text.is_empty() {
                    self.apply_intent(EditIntent::PasteText { text }, window, cx);
                }
            }
        }
    }

    pub(crate) fn toggle_bold(
        &mut self,
        _: &ToggleBold,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_intent(EditIntent::ToggleMark { mark: Mark::Bold }, window, cx);
    }

    pub(crate) fn toggle_italic(
        &mut self,
        _: &ToggleItalic,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_intent(EditIntent::ToggleMark { mark: Mark::Italic }, window, cx);
    }

    pub(crate) fn toggle_code(
        &mut self,
        _: &ToggleCode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_intent(EditIntent::ToggleMark { mark: Mark::Code }, window, cx);
    }

    pub(crate) fn toggle_underline(
        &mut self,
        _: &ToggleUnderline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_intent(
            EditIntent::ToggleMark {
                mark: Mark::Underline,
            },
            window,
            cx,
        );
    }

    pub(crate) fn toggle_strike(
        &mut self,
        _: &ToggleStrike,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_intent(EditIntent::ToggleMark { mark: Mark::Strike }, window, cx);
    }

    // ---- focus routing ----

    fn focused_child(&self, window: &Window, cx: &App) -> Option<Entity<ParagraphView>> {
        self.children
            .iter()
            .find(|(_, view)| view.read(cx).focus_handle(cx).is_focused(window))
            .map(|(_, view)| view.clone())
    }

    pub(crate) fn focused_child_composing(&self, window: &Window, cx: &App) -> bool {
        self.focused_child(window, cx)
            .map(|view| view.read(cx).is_composing())
            .unwrap_or(false)
    }

    fn focused_node_kind(&self) -> Option<NodeKind> {
        let session = self.session.borrow();
        let DocumentPosition::Text(point) = session.selection().focus() else {
            return None;
        };
        session
            .document()
            .node(point.node_id())
            .map(|node| node.kind().clone())
    }

    /// Moves platform focus to the block holding the selection focus.
    pub(crate) fn route_focus(&self, window: &mut Window, cx: &App) {
        let node = match self.session.borrow().selection().focus() {
            DocumentPosition::Text(point) => point.node_id(),
            DocumentPosition::Gap(_) => return,
        };
        if let Some((_, view)) = self.children.iter().find(|(id, _)| *id == node) {
            let handle = view.read(cx).focus_handle(cx);
            window.focus(&handle);
        }
    }

    /// Builds children if needed and routes focus; used once at window open.
    pub(crate) fn route_focus_initial(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_children(cx);
        let app = &*cx;
        self.route_focus(window, app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::HeadingLevel;

    #[test]
    fn ordinary_enter_splits_but_code_block_enter_inserts_lf() {
        assert_eq!(
            enter_plan_for_kind(&NodeKind::Paragraph),
            EnterPlan::SplitBlock
        );
        assert_eq!(
            enter_plan_for_kind(&NodeKind::Heading(HeadingLevel::new(2).unwrap())),
            EnterPlan::SplitBlock
        );
        assert_eq!(
            enter_plan_for_kind(&NodeKind::CodeBlock),
            EnterPlan::InsertLineBreak
        );
    }

    #[test]
    fn collapsed_caret_at_ordinary_block_start_converts_to_list() {
        assert_eq!(
            tab_plan_for_block(&NodeKind::Paragraph, false, true, 0),
            Some(TabPlan::ConvertToList)
        );
    }

    #[test]
    fn mid_paragraph_and_selection_insert_a_soft_tab() {
        assert_eq!(
            tab_plan_for_block(&NodeKind::Paragraph, false, true, 1),
            Some(TabPlan::InsertSoftTab)
        );
        assert_eq!(
            tab_plan_for_block(&NodeKind::Paragraph, false, false, 0),
            Some(TabPlan::InsertSoftTab)
        );
        assert_eq!(SOFT_TAB, "    ");
        assert!(!SOFT_TAB.contains('\t'));
    }

    #[test]
    fn code_block_tab_is_text_indent_even_inside_a_list() {
        assert_eq!(
            tab_plan_for_block(&NodeKind::CodeBlock, true, true, 0),
            Some(TabPlan::InsertSoftTab)
        );
        assert_eq!(
            tab_plan_for_block(&NodeKind::CodeBlock, false, false, 4),
            Some(TabPlan::InsertSoftTab)
        );
    }

    #[test]
    fn ordinary_list_item_keeps_structural_tab() {
        assert_eq!(
            tab_plan_for_block(&NodeKind::Paragraph, true, true, 0),
            None
        );
    }
}
