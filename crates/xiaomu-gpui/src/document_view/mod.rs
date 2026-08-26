//! Multi-block document view: the block list container.
//!
//! Owns the shared [`DocumentSession`] handle and renders one
//! [`ParagraphView`] per inline-bearing block in document order inside a
//! scrollable column. All keyboard actions are registered here and bubble up
//! from the focused block; cross-block navigation translates keys into
//! document positions via pure logic ([`navigation`]) applied as runtime
//! [`EditIntent::SetSelection`]s. Mouse drag selection hit-tests against the
//! per-block bounds published during paint.
//!
//! Kind-driven visual distinction lives in [`Self::render_block_tree`]:
//! headings scale with their level, quote descendants are indented behind a
//! bar with muted text, list items indent per nesting depth.

pub(crate) mod cache_key;
pub(crate) mod navigation;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    App, Context, Entity, Focusable as _, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Window, div, prelude::*, px,
};

use xiaomu_core::document::{Mark, NodeContent, NodeId, NodeKind};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::clipboard::{TextClipboard, normalize_paste_text};
use xiaomu_runtime::session::{DocumentPosition, EditIntent};

use crate::block_view::{
    Backspace, BlockBoundsRegistry, ClipboardCopy, ClipboardCut, ClipboardPaste, Delete, Down, End,
    Enter, Home, Left, ParagraphView, Redo, Right, SelectAll, SelectDown, SelectEnd, SelectHome,
    SelectLeft, SelectRight, SelectUp, SharedSession, ShiftTabIndent, TabIndent, ToggleBold,
    ToggleCode, ToggleItalic, ToggleStrike, ToggleUnderline, Undo, Up,
};
use crate::input::platform_clipboard::PlatformClipboard;

/// One navigation step direction for the caret focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavStep {
    /// One scalar left, wrapping into the previous block at the start.
    Left,
    /// One scalar right, wrapping into the next block at the end.
    Right,
    /// One visual line up (the previous block in this slice).
    Up,
    /// One visual line down (the next block in this slice).
    Down,
    /// Logical start of the current block.
    LineStart,
    /// Logical end of the current block.
    LineEnd,
}

/// A multi-block editor view over one shared session.
pub struct DocumentView {
    session: SharedSession,
    /// Render generation: bumped on every edit so block layout caches
    /// invalidate.
    epoch: Rc<Cell<u64>>,
    registry: BlockBoundsRegistry,
    /// Child views keyed by their inline node, kept alive across renders so
    /// IME composition and focus state survive unrelated re-renders.
    children: Vec<(NodeId, Entity<ParagraphView>)>,
    is_dragging: bool,
}

impl DocumentView {
    /// Creates the document view over a shared session.
    #[must_use]
    pub fn new(session: SharedSession) -> Self {
        Self {
            session,
            epoch: Rc::new(Cell::new(0)),
            registry: Rc::new(RefCell::new(Vec::new())),
            children: Vec::new(),
            is_dragging: false,
        }
    }

    /// Returns the shared session this view renders.
    #[must_use]
    pub fn session(&self) -> &SharedSession {
        &self.session
    }

    // ---- central intent application ----

    /// Applies one editing intent centrally: guards the composing block,
    /// bumps the render epoch on change, and routes platform focus to the
    /// block holding the selection focus afterwards.
    fn apply_intent(&mut self, intent: EditIntent, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused_child_composing(window, cx) {
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        let outcome = self.session.borrow_mut().apply_intent(&intent);
        match outcome {
            Ok(outcome) => {
                if outcome != xiaomu_runtime::session::SessionOutcome::NoChange {
                    self.epoch.set(self.epoch.get() + 1);
                }
                if outcome == xiaomu_runtime::session::SessionOutcome::DocumentChanged {
                    self.route_focus(window, cx);
                }
                cx.notify();
            }
            Err(error) => eprintln!("xiaomu: intent rejected: {error}"),
        }
    }

    /// Places the selection endpoints absolutely, routing focus afterwards.
    fn set_selection(
        &mut self,
        anchor: TextPoint,
        focus: TextPoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let intent = EditIntent::SetSelection { anchor, focus };
        let outcome = self.session.borrow_mut().apply_intent(&intent);
        match outcome {
            Ok(xiaomu_runtime::session::SessionOutcome::NoChange) => {}
            Ok(_) => self.route_focus(window, cx),
            Err(error) => eprintln!("xiaomu: selection rejected: {error}"),
        }
        cx.notify();
    }

    /// Collapses the caret onto `point`.
    fn place(&mut self, point: TextPoint, window: &mut Window, cx: &mut Context<Self>) {
        self.set_selection(point, point, window, cx);
    }

    /// Moves the focus endpoint to `point`; keeps the current text anchor
    /// when `extend` is set. A gap anchor collapses onto the target (all
    /// endpoints are textual in this slice).
    fn move_focus_to(
        &mut self,
        point: TextPoint,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = if extend {
            match self.session.borrow().selection().anchor() {
                DocumentPosition::Text(point) => Some(point),
                DocumentPosition::Gap(_) => None,
            }
        } else {
            None
        };
        match anchor {
            Some(anchor) => self.set_selection(anchor, point, window, cx),
            None => self.place(point, window, cx),
        }
    }

    // ---- navigation ----

    /// Resolves the current focus as `(blocks, block index, raw byte)`.
    fn focus_location(&self) -> Option<(Vec<navigation::TextBlock>, usize, usize)> {
        let session = self.session.borrow();
        let blocks = navigation::text_blocks(session.document());
        let focus = match session.selection().focus() {
            DocumentPosition::Text(point) => point,
            DocumentPosition::Gap(_) => return None,
        };
        let index = navigation::block_index(&blocks, focus.node_id())?;
        Some((blocks, index, focus.offset().as_usize()))
    }

    /// Translates one navigation step into a selection update.
    fn navigate(
        &mut self,
        step: NavStep,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focused_child_composing(window, cx) {
            return;
        }
        let Some((blocks, index, offset)) = self.focus_location() else {
            return;
        };
        let target = match step {
            NavStep::Left => navigation::step_horizontal(&blocks, index, offset, false),
            NavStep::Right => navigation::step_horizontal(&blocks, index, offset, true),
            NavStep::Up => navigation::step_vertical(&blocks, index, offset, false),
            NavStep::Down => navigation::step_vertical(&blocks, index, offset, true),
            NavStep::LineStart => navigation::line_edge(&blocks, index, false),
            NavStep::LineEnd => navigation::line_edge(&blocks, index, true),
        };
        let Some((block, raw)) = target else {
            return;
        };
        let Some(offset) = navigation::validated_offset(&blocks[block], raw) else {
            return;
        };
        let point = TextPoint::new(blocks[block].node, offset, CursorAffinity::Before);
        self.move_focus_to(point, extend, window, cx);
    }

    // ---- action listeners ----

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Left, false, window, cx);
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Right, false, window, cx);
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Up, false, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Down, false, window, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Left, true, window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Right, true, window, cx);
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Up, true, window, cx);
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::Down, true, window, cx);
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::LineStart, false, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::LineEnd, false, window, cx);
    }

    fn select_home(&mut self, _: &SelectHome, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::LineStart, true, window, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavStep::LineEnd, true, window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
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

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::SplitBlock, window, cx);
    }

    fn tab_indent(&mut self, _: &TabIndent, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::IndentListItem, window, cx);
    }

    fn shift_tab_indent(
        &mut self,
        _: &ShiftTabIndent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_intent(EditIntent::OutdentListItem, window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::Backspace, window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::Delete, window, cx);
    }

    fn undo_entry(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let outcome = self.session.borrow_mut().undo();
        self.after_history(outcome, cx);
    }

    fn redo_entry(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
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

    fn copy(&mut self, _: &ClipboardCopy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.session.borrow().selected_text() {
            PlatformClipboard::new(&*cx).write_text(text);
        }
    }

    fn cut(&mut self, _: &ClipboardCut, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.session.borrow().selected_text();
        let Some(text) = selected else {
            return;
        };
        PlatformClipboard::new(&*cx).write_text(text);
        // A non-collapsed selection deletes as a whole; one undo unit.
        self.apply_intent(EditIntent::Delete, window, cx);
    }

    fn paste(&mut self, _: &ClipboardPaste, window: &mut Window, cx: &mut Context<Self>) {
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

    fn toggle_bold(&mut self, _: &ToggleBold, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::ToggleMark { mark: Mark::Bold }, window, cx);
    }

    fn toggle_italic(&mut self, _: &ToggleItalic, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::ToggleMark { mark: Mark::Italic }, window, cx);
    }

    fn toggle_code(&mut self, _: &ToggleCode, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::ToggleMark { mark: Mark::Code }, window, cx);
    }

    fn toggle_underline(
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

    fn toggle_strike(&mut self, _: &ToggleStrike, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::ToggleMark { mark: Mark::Strike }, window, cx);
    }

    // ---- focus routing ----

    fn focused_child(&self, window: &Window, cx: &App) -> Option<Entity<ParagraphView>> {
        self.children
            .iter()
            .find(|(_, view)| view.read(cx).focus_handle(cx).is_focused(window))
            .map(|(_, view)| view.clone())
    }

    fn focused_child_composing(&self, window: &Window, cx: &App) -> bool {
        self.focused_child(window, cx)
            .map(|view| view.read(cx).is_composing())
            .unwrap_or(false)
    }

    /// Moves platform focus to the block holding the selection focus.
    fn route_focus(&self, window: &mut Window, cx: &App) {
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

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_dragging = true;
        if let Some(point) = self.hit_test(event.position, cx) {
            if event.modifiers.shift {
                self.move_focus_to(point, true, window, cx);
            } else {
                self.place(point, window, cx);
            }
        }
    }

    fn on_mouse_move(
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

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
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

    // ---- rendering ----

    /// Syncs child entities to the current snapshot's block list, dropping
    /// views whose nodes no longer exist.
    fn sync_children(&mut self, cx: &mut Context<Self>) {
        let nodes: Vec<NodeId> = {
            let session = self.session.borrow();
            navigation::text_blocks(session.document())
                .into_iter()
                .map(|block| block.node)
                .collect()
        };

        let mut pool = std::mem::take(&mut self.children);
        let session = self.session.clone();
        let epoch = self.epoch.clone();
        let registry = self.registry.clone();
        self.children = nodes
            .into_iter()
            .map(|node| {
                if let Some(position) = pool.iter().position(|(id, _)| *id == node) {
                    pool.remove(position)
                } else {
                    let view = cx.new(|cx| {
                        ParagraphView::new(
                            session.clone(),
                            epoch.clone(),
                            registry.clone(),
                            node,
                            cx,
                        )
                    });
                    (node, view)
                }
            })
            .collect();
        // Stale entries dropped with `pool`.
    }

    /// Renders the document tree with kind-driven styling.
    fn render_block_tree(
        &self,
        id: NodeId,
        in_quote: bool,
        list_depth: usize,
        index: usize,
    ) -> gpui::AnyElement {
        let node_data = {
            let session = self.session.borrow();
            session
                .document()
                .node(id)
                .map(|node| (node.kind().clone(), node.content().clone()))
        };
        let Some((kind, content)) = node_data else {
            return div().into_any_element();
        };

        match content {
            NodeContent::Inline(_) => {
                let Some((_, view)) = self.children.iter().find(|(child, _)| *child == id) else {
                    return div().into_any_element();
                };
                style_block(view.clone(), &kind, in_quote, list_depth, index).into_any_element()
            }
            NodeContent::Children(children) => {
                let next_quote = in_quote || matches!(kind, NodeKind::Quote);
                let next_depth = list_depth
                    + usize::from(matches!(kind, NodeKind::BulletList | NodeKind::OrderedList));
                let mut column = div().flex().flex_col();
                if matches!(kind, NodeKind::Quote) {
                    column = column.border_l_2().border_color(gpui::black()).pl_4();
                }
                for (child_index, child) in children.into_iter().enumerate() {
                    column = column.child(self.render_block_tree(
                        child,
                        next_quote,
                        next_depth,
                        index + child_index,
                    ));
                }
                column.into_any_element()
            }
            NodeContent::Atomic => div()
                .h(px(1.0))
                .w_full()
                .bg(gpui::rgba(0xccccccff))
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }
}

/// Wraps one block view with kind-driven visual styling.
fn style_block(
    view: Entity<ParagraphView>,
    kind: &NodeKind,
    in_quote: bool,
    list_depth: usize,
    index: usize,
) -> gpui::Stateful<gpui::Div> {
    let mut block = div().id(index).w_full();
    if let NodeKind::Heading(level) = kind {
        let scale = match level.as_u8() {
            1 => 1.6,
            2 => 1.35,
            _ => 1.15,
        };
        block = block
            .text_size(px(20.0 * scale))
            .font_weight(gpui::FontWeight::BOLD);
    }
    if in_quote {
        block = block.text_color(gpui::rgba(0x444444ff));
    }
    if list_depth > 0 {
        block = block.ml(px(24.0 * list_depth as f32));
    }
    block.child(view)
}

impl Render for DocumentView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_children(cx);

        let root = self.session.borrow().document().root();

        // Each paint pass repopulates the registry; stale entries must go.
        self.registry.borrow_mut().clear();

        let tree = self.render_block_tree(root, false, 0, 0);

        div()
            .key_context("XiaomuDocument")
            .size_full()
            .bg(gpui::white())
            .p_4()
            .line_height(px(28.0))
            .text_size(px(20.0))
            .text_color(gpui::black())
            .cursor(gpui::CursorStyle::IBeam)
            .id("xiaomu-document-scroll")
            .overflow_y_scroll()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::tab_indent))
            .on_action(cx.listener(Self::shift_tab_indent))
            .on_action(cx.listener(Self::undo_entry))
            .on_action(cx.listener(Self::redo_entry))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::toggle_bold))
            .on_action(cx.listener(Self::toggle_italic))
            .on_action(cx.listener(Self::toggle_code))
            .on_action(cx.listener(Self::toggle_underline))
            .on_action(cx.listener(Self::toggle_strike))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(tree)
    }
}
