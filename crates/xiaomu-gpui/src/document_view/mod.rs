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

pub(crate) mod actions;
pub(crate) mod cache_key;
pub(crate) mod navigation;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{Context, Entity, MouseButton, Window, div, prelude::*, px};

use xiaomu_core::document::{NodeContent, NodeId, NodeKind};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::session::{DocumentPosition, EditIntent};

use xiaomu_runtime::persistence::DocumentPersistence;

use crate::block_view::{BlockBoundsRegistry, ParagraphView, SharedSession};

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
    /// Host adapter for the create → load → edit → save contract; absent
    /// when the host persists through its own channel.
    persistence: Option<Rc<RefCell<dyn DocumentPersistence>>>,
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
            persistence: None,
        }
    }

    /// Attaches the host persistence adapter (Ctrl/Cmd-S saves).
    pub fn set_persistence(&mut self, persistence: Rc<RefCell<dyn DocumentPersistence>>) {
        self.persistence = Some(persistence);
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
                } else if actions::is_structural(&intent) {
                    // Structural no-ops are position-dependent (first item,
                    // top-level item); surface them together with where the
                    // session thinks the caret is, so real-machine testing
                    // can tell "no-op here" from "key not delivered".
                    let where_am_i = {
                        let session = self.session.borrow();
                        match session.selection().focus() {
                            DocumentPosition::Text(point) => {
                                let kind = session
                                    .document()
                                    .node(point.node_id())
                                    .map(|n| format!("{:?}", n.kind()))
                                    .unwrap_or_else(|| "<unknown>".into());
                                let parent = session
                                    .document()
                                    .parent_of(point.node_id())
                                    .and_then(|p| session.document().node(p))
                                    .map(|n| format!("{:?}", n.kind()))
                                    .unwrap_or_else(|| "<none>".into());
                                format!("caret in {kind} (parent {parent})")
                            }
                            DocumentPosition::Gap(_) => "caret at a gap".to_owned(),
                        }
                    };
                    eprintln!(
                        "xiaomu: structural command has no effect here [{where_am_i}]: {intent:?}"
                    );
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
            .on_action(cx.listener(Self::save_document))
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
