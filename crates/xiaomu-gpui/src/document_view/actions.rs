//! Action listeners and mouse handlers of [`DocumentView`].
//!
//! Split out of `mod.rs` to stay under the source-size guardrail. All
//! handlers translate GPUI events into runtime intents or pure navigation
//! steps; the session remains the single mutation point.

use gpui::{
    App, Context, Entity, Focusable as _, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Window,
};

use xiaomu_core::document::{Mark, NodeId};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::clipboard::normalize_paste_text;
use xiaomu_runtime::session::{DocumentPosition, EditIntent};

use crate::block_view::{
    Backspace, ClipboardCopy, ClipboardCut, ClipboardPaste, Delete, Down, End, Enter, Home, Left,
    Redo, Right, SaveDocument, SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft,
    SelectRight, SelectUp, ShiftTabIndent, TabIndent, ToggleBold, ToggleCode, ToggleItalic,
    ToggleStrike, ToggleUnderline, Undo, Up,
};
use xiaomu_runtime::clipboard::TextClipboard;

use crate::block_view::ParagraphView;
use crate::input::platform_clipboard::PlatformClipboard;

use super::{DocumentView, NavStep, navigation};

/// Soft tab inserted when Tab is pressed inside a plain block away from
/// offset 0. A literal U+0009 has no glyph advance in GPUI's shaper, so it
/// looks like a no-op; four ASCII spaces match the usual editor tab size
/// without needing tab-stop layout. Two spaces is a common indent unit,
/// not a tab stop.
const SOFT_TAB: &str = "    ";

/// What Tab should do given the caret context of a plain block. A caret
/// inside a list never reaches this decision and keeps the item-indent
/// intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabPlan {
    /// Collapsed caret at the block start: gesture for "make this a list".
    ConvertToList,
    /// Anywhere else: insert a visible soft tab (four spaces).
    InsertSoftTab,
}

fn tab_plan_for_plain_block(collapsed: bool, offset: usize) -> TabPlan {
    if collapsed && offset == 0 {
        TabPlan::ConvertToList
    } else {
        TabPlan::InsertSoftTab
    }
}

/// Whether an intent is a structural command whose no-op is worth surfacing.
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
        self.apply_intent(EditIntent::SplitBlock, window, cx);
    }

    pub(crate) fn tab_indent(
        &mut self,
        _: &TabIndent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Intuitive Tab semantics for plain blocks: only a collapsed caret
        // at the very start gestures "turn this block into a list"; anywhere
        // else Tab inserts a visible soft tab (replacing the selection).
        // Inside a list, items with a previous sibling indent as before.
        let plan = {
            let session = self.session.borrow();
            match session.selection().focus() {
                DocumentPosition::Text(point) => {
                    let in_list =
                        navigation::list_context(session.document(), point.node_id()).is_some();
                    if in_list {
                        None
                    } else {
                        session.text_selection().map(|selection| {
                            tab_plan_for_plain_block(
                                selection.is_collapsed(),
                                selection.focus().offset().as_usize(),
                            )
                        })
                    }
                }
                DocumentPosition::Gap(_) => None,
            }
        };
        match plan {
            Some(TabPlan::ConvertToList) => {
                self.apply_intent(
                    EditIntent::TurnInto {
                        kind: xiaomu_core::document::NodeKind::BulletList,
                    },
                    window,
                    cx,
                );
                return;
            }
            Some(TabPlan::InsertSoftTab) => {
                self.apply_intent(
                    EditIntent::InsertText {
                        text: SOFT_TAB.to_owned(),
                    },
                    window,
                    cx,
                );
                return;
            }
            _ => {}
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
        // level; a top-level item lifts back to a plain paragraph.
        let lifts_out = {
            let session = self.session.borrow();
            match session.selection().focus() {
                DocumentPosition::Text(point) => matches!(
                    navigation::list_context(session.document(), point.node_id()),
                    Some(context) if !context.nested
                ),
                DocumentPosition::Gap(_) => false,
            }
        };
        if lifts_out {
            self.apply_intent(
                EditIntent::TurnInto {
                    kind: xiaomu_core::document::NodeKind::Paragraph,
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
            Ok(()) => eprintln!("xiaomu: snapshot saved"),
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
        if let Some(text) = self.session.borrow().selected_text() {
            PlatformClipboard::new(&*cx).write_text(text);
        }
    }

    pub(crate) fn cut(&mut self, _: &ClipboardCut, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.session.borrow().selected_text();
        let Some(text) = selected else {
            return;
        };
        PlatformClipboard::new(&*cx).write_text(text);
        // A non-collapsed selection deletes as a whole; one undo unit.
        self.apply_intent(EditIntent::Delete, window, cx);
    }

    pub(crate) fn paste(
        &mut self,
        _: &ClipboardPaste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = PlatformClipboard::new(&*cx).read_text() else {
            return;
        };
        // Line breaks cannot exist inside inline text; pasted breaks become
        // spaces. Empty text must not clear the selection.
        let text = normalize_paste_text(&text);
        if !text.is_empty() {
            self.apply_intent(
                EditIntent::InsertText {
                    text: text.to_owned(),
                },
                window,
                cx,
            );
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

    // ---- mouse ----

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_dragging = true;
        if let Some(point) = self.hit_test(event.position, cx) {
            // Click-placement diagnostic: shows which block the click landed
            // on, so mis-hits are visible during real-machine testing.
            let clicked = {
                let session = self.session.borrow();
                session.document().node(point.node_id()).map(|node| {
                    let text = node
                        .content()
                        .as_inline()
                        .map(|inline| {
                            let text: String = inline
                                .runs()
                                .iter()
                                .map(|run| run.text().as_str())
                                .collect();
                            let preview: String = text.chars().take(8).collect();
                            format!(" \u{201c}{preview}\u{201d}")
                        })
                        .unwrap_or_default();
                    format!(
                        "{:?}{text} at byte {}",
                        node.kind(),
                        point.offset().as_usize()
                    )
                })
            };
            if let Some(description) = clicked {
                eprintln!("xiaomu: click placed caret in {description}");
            }
            if event.modifiers.shift {
                self.move_focus_to(point, true, window, cx);
            } else {
                self.place(point, window, cx);
            }
        }
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_dragging {
            return;
        }
        if let Some(point) = self.hit_test(event.position, cx) {
            self.move_focus_to(point, true, window, cx);
        }
    }

    pub(crate) fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_dragging = false;
    }

    /// Maps a window-space point to a validated caret point via the paint
    /// registry: nearest block by vertical position, then x hit-testing
    /// within that block's shaped line.
    fn hit_test(&self, position: Point<Pixels>, cx: &App) -> Option<TextPoint> {
        let registry = self.registry.borrow();
        let mut nearest: Option<(NodeId, Pixels)> = None;
        for (node, bounds) in registry.iter() {
            let distance = if position.y < bounds.top() {
                bounds.top() - position.y
            } else if position.y > bounds.bottom() {
                position.y - bounds.bottom()
            } else {
                Pixels::ZERO
            };
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((*node, distance));
            }
        }
        let (node, _) = nearest?;

        let session = self.session.borrow();
        let blocks = navigation::text_blocks(session.document());
        drop(session);
        let block = blocks.iter().find(|block| block.node == node)?;
        let child = self
            .children
            .iter()
            .find(|(id, _)| *id == node)
            .map(|(_, view)| view.clone())?;
        let raw = child.read(cx).hit_test_x(position.x)?;
        let clamped = raw.min(block.text().len());
        let offset = navigation::validated_offset(block, clamped)
            .or_else(|| navigation::validated_offset(block, block.text().len()))?;
        Some(TextPoint::new(node, offset, CursorAffinity::Before))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_caret_at_block_start_converts_to_list() {
        assert_eq!(tab_plan_for_plain_block(true, 0), TabPlan::ConvertToList);
    }

    #[test]
    fn mid_paragraph_and_selection_insert_a_soft_tab() {
        assert_eq!(tab_plan_for_plain_block(true, 1), TabPlan::InsertSoftTab);
        assert_eq!(tab_plan_for_plain_block(false, 0), TabPlan::InsertSoftTab);
        assert_eq!(SOFT_TAB, "    ");
        assert!(!SOFT_TAB.contains('\t'));
    }
}
