//! Multi-block document view: the block list container.
//!
//! Owns the shared [`DocumentSession`] handle and renders one
//! [`ParagraphView`] per inline-bearing block in document order inside a
//! scrollable column. Keyboard navigation translates visual gestures through
//! the most recent block layouts and applies the resulting Core positions as
//! runtime [`EditIntent::SetSelection`]s. Mouse drag selection hit-tests
//! against the per-block bounds published during paint and back-projects
//! renderer display bytes into exact mixed-inline positions through
//! [`DocumentSession::set_inline_selection`].
//!
//! Kind-driven visual distinction lives in [`Self::render_block_tree`]:
//! headings scale with their level, quote descendants are indented behind a
//! bar with muted text, list items indent per nesting depth and show a projected bullet or ordinal marker.

pub(crate) mod actions;
pub(crate) mod cache_key;
pub(crate) mod markers;
pub(crate) mod mouse;
pub(crate) mod navigation;
mod visual_navigation;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    App, Context, Entity, Focusable as _, MouseButton, Pixels, ScrollHandle, Window, div,
    prelude::*, px,
};

use xiaomu_core::document::{NodeContent, NodeId, NodeKind};
use xiaomu_core::selection::{InlinePoint, TextPoint};
use xiaomu_runtime::session::{DocumentPosition, EditIntent};

use xiaomu_runtime::persistence::DocumentPersistence;

use crate::accessibility::{AccessibilityProjection, project_accessibility};
use crate::block_view::{BlockBoundsRegistry, ParagraphView, SharedSession};
use crate::inline_atom::InlineAtomRendererRegistry;
use visual_navigation::NavStep;

/// A multi-block editor view over one shared session.
pub struct DocumentView {
    session: SharedSession,
    /// Render generation: bumped on every edit so block layout caches
    /// invalidate.
    epoch: Rc<Cell<u64>>,
    registry: BlockBoundsRegistry,
    /// Shared viewport scroll state. Focused blocks use it to keep the caret
    /// visible without leaking viewport geometry into Core or runtime.
    scroll_handle: ScrollHandle,
    /// Child views keyed by their inline node, kept alive across renders so
    /// IME composition and focus state survive unrelated re-renders.
    children: Vec<(NodeId, Entity<ParagraphView>)>,
    is_dragging: bool,
    /// The focus point produced by the previous vertical move plus the x
    /// column to preserve. Pairing x with its anchor makes direct IME/text
    /// edits invalidate stale vertical-navigation state automatically.
    desired_x: Option<(InlinePoint, Pixels)>,
    /// Host adapter for the create → load → edit → save contract; absent
    /// when the host persists through its own channel.
    persistence: Option<Rc<RefCell<dyn DocumentPersistence>>>,
    /// Host-registered inline-atom renderers; kinds without an entry keep
    /// the deterministic fallback display.
    atom_renderers: Rc<InlineAtomRendererRegistry>,
}

impl DocumentView {
    /// Creates the document view over one shared session.
    #[must_use]
    pub fn new(session: SharedSession) -> Self {
        Self {
            session,
            epoch: Rc::new(Cell::new(0)),
            registry: Rc::new(RefCell::new(Vec::new())),
            scroll_handle: ScrollHandle::new(),
            children: Vec::new(),
            is_dragging: false,
            desired_x: None,
            persistence: None,
            atom_renderers: Rc::new(InlineAtomRendererRegistry::new()),
        }
    }

    /// Attaches the host persistence adapter (Ctrl/Cmd-S saves).
    pub fn set_persistence(&mut self, persistence: Rc<RefCell<dyn DocumentPersistence>>) {
        self.persistence = Some(persistence);
    }

    /// Attaches the host's inline-atom renderer registry.
    ///
    /// Kinds without a registered renderer keep the deterministic fallback
    /// (display and read as `fallback_text`), so partial registration can
    /// never drop atomic content.
    pub fn set_atom_renderers(&mut self, renderers: Rc<InlineAtomRendererRegistry>) {
        self.atom_renderers = renderers;
    }

    /// Returns the view's inline-atom renderer registry.
    #[must_use]
    pub fn atom_renderers(&self) -> &InlineAtomRendererRegistry {
        &self.atom_renderers
    }

    /// Returns the shared session this view renders.
    #[must_use]
    pub fn session(&self) -> &SharedSession {
        &self.session
    }

    /// Projects the current canonical accessibility state and real focus owner.
    ///
    /// Selection and focus deliberately remain separate. An inactive editor
    /// can retain its canonical caret while reporting no `focus_owner`. Child
    /// views are materialized by render / focus restoration before a focus
    /// owner can be reported.
    #[must_use]
    pub fn accessibility_projection(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<AccessibilityProjection> {
        let focus_owner = if window.is_window_active() {
            self.children
                .iter()
                .find(|(_, view)| view.read(cx).focus_handle(cx).is_focused(window))
                .map(|(node, _)| *node)
        } else {
            None
        };
        let session = self.session.borrow();
        project_accessibility(session.document(), session.selection(), focus_owner)
    }

    /// Restores native keyboard focus to the block holding selection focus.
    ///
    /// Hosts call this after mounting an [`EditorInstance`](crate::editor::EditorInstance)
    /// whose [`DocumentSelection`](xiaomu_runtime::session::DocumentSelection)
    /// was restored from host state. The canonical selection is not changed.
    pub fn focus_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_children(cx);
        self.route_focus(window, cx);
    }

    // ---- central intent application ----

    /// Applies one editing intent centrally: guards the composing block,
    /// bumps the render epoch on change, and routes platform focus to the
    /// block holding the selection focus afterwards.
    fn apply_intent(&mut self, intent: EditIntent, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused_child_composing(window, cx) {
            #[cfg(debug_assertions)]
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        self.desired_x = None;
        let outcome = self.session.borrow_mut().apply_intent(&intent);
        match outcome {
            Ok(outcome) => {
                if outcome != xiaomu_runtime::session::SessionOutcome::NoChange {
                    self.epoch.set(self.epoch.get() + 1);
                }
                if outcome == xiaomu_runtime::session::SessionOutcome::DocumentChanged {
                    // Structural edits such as SplitBlock may move the
                    // selection onto a newly-created node. Materialize the
                    // matching ParagraphView before trying to transfer native
                    // focus; otherwise the old block remains focused while the
                    // document selection already points at the new block.
                    self.sync_children(cx);
                    self.route_focus(window, cx);
                    self.request_focus_scroll(cx);
                }
                #[cfg(debug_assertions)]
                if outcome != xiaomu_runtime::session::SessionOutcome::DocumentChanged
                    && actions::is_structural(&intent)
                {
                    // Structural no-ops are position-dependent (first item,
                    // top-level item); surface them together with where the
                    // session thinks the caret is, so real-machine testing
                    // can tell "no-op here" from "key not delivered".
                    let where_am_i = {
                        let session = self.session.borrow();
                        match session.selection().focus() {
                            DocumentPosition::Inline(point) => {
                                let describe = |id| {
                                    session
                                        .document()
                                        .node(id)
                                        .map(|n| {
                                            let text = n
                                                .content()
                                                .as_inline()
                                                .map(|inline| {
                                                    let text: String = inline
                                                        .runs()
                                                        .iter()
                                                        .map(|run| run.text().as_str())
                                                        .collect();
                                                    let preview: String =
                                                        text.chars().take(8).collect();
                                                    format!(" \u{201c}{preview}\u{201d}")
                                                })
                                                .unwrap_or_default();
                                            format!("{:?}{text}", n.kind())
                                        })
                                        .unwrap_or_else(|| "<unknown>".into())
                                };
                                let kind = describe(point.node_id());
                                let parent = session
                                    .document()
                                    .parent_of(point.node_id())
                                    .map(describe)
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
    ///
    /// Text-only compatibility entry for keyboard flows whose positions are
    /// canonical `TextPoint`s; mouse placement routes through
    /// [`Self::set_inline_selection`] so atom ordinals survive.
    fn set_selection(
        &mut self,
        anchor: TextPoint,
        focus: TextPoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_inline_selection(anchor.into(), focus.into(), window, cx);
    }

    /// Places the selection endpoints at exact mixed-inline positions,
    /// routing focus afterwards. Same-boundary atom gaps that have no
    /// text-only projection are preserved verbatim.
    fn set_inline_selection(
        &mut self,
        anchor: InlinePoint,
        focus: InlinePoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.desired_x = None;
        let outcome = self
            .session
            .borrow_mut()
            .set_inline_selection(anchor, focus);
        match outcome {
            Ok(xiaomu_runtime::session::SessionOutcome::NoChange) => {}
            Ok(_) => {
                self.route_focus(window, cx);
                self.request_focus_scroll(cx);
            }
            Err(error) => eprintln!("xiaomu: selection rejected: {error}"),
        }
        cx.notify();
    }

    /// Collapses the caret onto `point`.
    fn place(&mut self, point: InlinePoint, window: &mut Window, cx: &mut Context<Self>) {
        self.set_inline_selection(point, point, window, cx);
    }

    /// Moves the focus endpoint to `point`; keeps the current mixed-inline
    /// anchor when `extend` is set. A gap anchor collapses onto the target.
    fn move_focus_to(
        &mut self,
        point: InlinePoint,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = if extend {
            match self.session.borrow().selection().anchor() {
                // Mixed-inline anchors keep their atom ordinal; keyboard and
                // pointer callers both route through here.
                DocumentPosition::Inline(point) => Some(point),
                DocumentPosition::Gap(_) => None,
            }
        } else {
            None
        };
        match anchor {
            Some(anchor) => self.set_inline_selection(anchor, point, window, cx),
            None => self.place(point, window, cx),
        }
    }

    /// Marks the block holding the document focus for one keep-visible pass.
    fn request_focus_scroll(&self, cx: &App) {
        let node = match self.session.borrow().selection().focus() {
            DocumentPosition::Inline(point) => point.node_id(),
            DocumentPosition::Gap(_) => return,
        };
        if let Some((_, view)) = self.children.iter().find(|(id, _)| *id == node) {
            view.read(cx).request_caret_scroll();
        }
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

        let scroll_handle = self.scroll_handle.clone();
        let atom_renderers = self.atom_renderers.clone();
        for (_, child) in &self.children {
            let scroll_handle = scroll_handle.clone();
            let atom_renderers = atom_renderers.clone();
            child.update(cx, |view, _| {
                view.attach_scroll_handle(scroll_handle);
                view.attach_atom_renderers(atom_renderers);
            });
        }
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
                let marker = {
                    let session = self.session.borrow();
                    markers::marker_for_block(session.document(), id)
                };
                markers::style_block(
                    view.clone(),
                    &kind,
                    in_quote,
                    list_depth,
                    marker.as_ref(),
                    index,
                )
                .into_any_element()
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
            .track_scroll(&self.scroll_handle)
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
            .on_action(cx.listener(Self::hard_break))
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
