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
mod block_tree;
pub(crate) mod cache_key;
pub(crate) mod cell_selection;
mod clipboard;
mod column_resize;
mod history_clock;
mod host_extensions;
mod host_selection_intent;
mod host_transaction;
pub(crate) mod markers;
mod measured_table;
#[cfg(test)]
mod measured_table_tests;
pub(crate) mod mouse;
pub(crate) mod navigation;
mod node_selection;
mod rejection;
mod scroll_tree;
pub use rejection::{EditorRejection, EditorRejectionReason, EditorRejectionStage};
mod table_block;
pub(crate) mod table_guard;
#[cfg(test)]
mod table_guard_tests;
mod table_scroll;
mod task_checkbox;
mod vertical_geometry;
mod visual_navigation;

#[cfg(test)]
mod command_composition_tests;
#[cfg(test)]
mod explicit_all_selection_tests;
#[cfg(all(test, target_os = "linux"))]
mod host_form_order_tests;
#[cfg(test)]
mod host_intent_tests;
#[cfg(test)]
mod list_marker_tests;
#[cfg(test)]
mod select_all_inline_atoms_tests;
#[cfg(test)]
mod task_checkbox_tests;
#[cfg(test)]
mod task_layout_tests;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{App, Context, Entity, Focusable as _, Pixels, ScrollHandle, Window, prelude::*};

use xiaomu_core::document::{ImageAttrs, ImageSource, NodeAttrs, NodeId, XiaomuDocument};
use xiaomu_core::selection::InlinePoint;
use xiaomu_runtime::session::{DocumentPosition, EditIntent};

use xiaomu_runtime::assets::AssetService;
use xiaomu_runtime::persistence::DocumentPersistence;

use crate::accessibility::{AccessibilityProjection, project_accessibility};
use crate::atom_capability::SharedAtomCapability;
use crate::block_view::{BlockBoundsRegistry, ParagraphView, SharedSession};
use crate::image_block::{ImageLoadCache, ImageLoadState, SharedImageLoadCache, sync_image_loads};
use crate::inline_atom::InlineAtomRendererRegistry;
use visual_navigation::NavStep;

/// A multi-block editor view over one shared session.
pub struct DocumentView {
    session: SharedSession,
    history_clock: Option<crate::history_clock::SharedHistoryClock>,
    /// Render generation: bumped on every edit so block layout caches
    /// invalidate.
    epoch: Rc<Cell<u64>>,
    registry: BlockBoundsRegistry,
    /// Full cell bounds, including padding and space below shorter content.
    cell_registry: BlockBoundsRegistry,
    /// Current measured viewport clips, separate from full navigation geometry.
    table_clips: table_scroll::TableClipRegistry,
    /// Per-view measured-table admission, also held by retained input handlers.
    table_capability: crate::table_capability::SharedTableCapability,
    cell_drag_anchor: Option<NodeId>,
    column_resize: column_resize::ColumnResizeState,
    range_input: Option<(NodeId, Entity<ParagraphView>)>,
    focus_handle: Option<gpui::FocusHandle>,
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
    atom_capability: Option<SharedAtomCapability>,
    /// Host-owned asset resolver for image blocks; absent means image
    /// placeholders stay neutral until a host attaches.
    asset_service: Option<Rc<dyn AssetService>>,
    /// Per-node image load states shared with resolve callbacks.
    image_loads: SharedImageLoadCache,
    command_router: Option<Rc<dyn crate::editor_commands::EditorCommandRouter>>,
    code_block_presentation: Option<crate::code_presentation::CodeBlockPresentation>,
    list_marker_provider: Option<Rc<dyn crate::list_marker::ListMarkerLabelProvider>>,
}

impl DocumentView {
    /// Creates an unstamped view over one shared session. Timed sessions
    /// isolate its edits; use `new_with_history_clock` to share a clock.
    #[must_use]
    pub fn new(session: SharedSession) -> Self {
        Self {
            session,
            history_clock: None,
            epoch: Rc::new(Cell::new(0)),
            registry: Rc::new(RefCell::new(Vec::new())),
            cell_registry: Rc::new(RefCell::new(Vec::new())),
            table_clips: Default::default(),
            table_capability: Rc::new(RefCell::new(Default::default())),
            cell_drag_anchor: None,
            column_resize: Default::default(),
            range_input: None,
            focus_handle: None,
            scroll_handle: ScrollHandle::new(),
            children: Vec::new(),
            is_dragging: false,
            desired_x: None,
            persistence: None,
            atom_renderers: Rc::new(InlineAtomRendererRegistry::new()),
            atom_capability: None,
            asset_service: None,
            image_loads: Rc::new(ImageLoadCache::default()),
            command_router: None,
            code_block_presentation: None,
            list_marker_provider: None,
        }
    }

    /// Attaches the host asset resolver for image blocks.
    pub fn set_asset_service(&mut self, service: Rc<dyn AssetService>) {
        self.asset_service = Some(service);
    }

    /// Opts this view into measured Header/span/shared-column layout.
    ///
    /// Disabled by default. Opt-in does not grant unconditional edit access:
    /// each table must have supported current attributes and a successful
    /// measured layout in this view. Unknown presentation values and layout
    /// failures retain a visible, protected placeholder. Automatic columns use
    /// the explicit native sizing policy, not browser intrinsic-width parity.
    /// Each measured table owns a hidden-scrollbar horizontal viewport; native
    /// horizontal wheels reveal overflow without changing column widths. This
    /// presentation does not depend on enabling column-resize callbacks.
    /// Hosts changing a mounted view should notify its context afterward.
    pub fn set_measured_table_layout(&mut self, enabled: bool) {
        self.column_resize.cancel();
        self.column_resize.clear_measurements();
        self.table_capability.borrow_mut().set_enabled(enabled);
    }

    /// Allows exact, visually inert metadata entries on measured table rows.
    ///
    /// The default empty map rejects every row attribute. Each actual row
    /// entry must match an allowed key and value exactly; absent entries are
    /// fine. The host owns their interpretation and wire encoding. This never
    /// interprets, removes or rewrites canonical attributes and does not relax
    /// table/cell presentation rules or enable measured layout by itself.
    /// Every call revokes this instance's existing measurement admission and
    /// requires a fresh layout, including for retained native input handlers.
    /// Hosts changing a mounted view should notify its context afterward.
    pub fn set_measured_table_row_metadata(&mut self, metadata: NodeAttrs) {
        self.column_resize.cancel();
        self.column_resize.clear_measurements();
        self.table_capability
            .borrow_mut()
            .set_row_metadata(metadata);
    }

    /// Captures this editor's current measured-table presentation admission.
    ///
    /// Hosts may call this without an App/Window before saving or preparing a
    /// normal edit, with the current installed document. A document without
    /// tables passes; otherwise every table must be opted in and successfully
    /// measured in this instance. Unsupported changes and measurement failure
    /// refuse the whole document, including edits outside the failing table.
    /// Ordinary typing preserves admission when the table's structural key is
    /// unchanged, even before another paint or an immediate leave-time flush.
    /// This reports the latest measurement, not proof that the current text or
    /// viewport has already painted. Call it for each operation; don't retain
    /// the returned boolean as a lasting permission.
    ///
    /// This is not storage authorization or schema validation. Hosts must keep
    /// their own ownership/CAS checks and let history restoration and subsequent
    /// measurement recover admission. Do not use it to probe uninstalled edit
    /// candidates or other notes: its cache belongs to this view's document.
    #[must_use]
    pub fn measured_table_presentation_guard(&self) -> Rc<dyn Fn(&XiaomuDocument) -> bool> {
        let capability = self.table_capability.clone();
        Rc::new(move |document| capability.borrow().permits_document(document))
    }

    /// Returns the load state of one image block, when a request exists.
    #[must_use]
    pub fn image_load_state(&self, node: NodeId) -> Option<ImageLoadState> {
        let document = self.session.borrow().document().clone();
        let node_data = document.node(node)?;
        let attrs = ImageAttrs::from_attrs(node_data.attrs()).ok()?;
        let source_key = match attrs.source() {
            ImageSource::AssetRef(value) => value.clone(),
            ImageSource::ExternalUrl(url) => url.clone(),
        };
        self.image_loads.fresh_state(node, &source_key)
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

    /// Installs the host adapter that receives atom activations.
    pub fn set_atom_capability(&mut self, capability: SharedAtomCapability) {
        self.atom_capability = Some(capability);
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

    /// Applies a host command through the same path as built-in edit actions.
    ///
    /// Toolbars and host controls should use this instead of editing the
    /// shared session directly. An active native composition ignores the
    /// command. Otherwise the session's policy runs normally; accepted edits
    /// invalidate layout, sync child views, restore focus and request caret
    /// scrolling using the built-in action behavior. Rejections are logged,
    /// emit [`EditorRejection`], and leave canonical session state unchanged.
    pub fn apply_edit_intent(
        &mut self,
        intent: EditIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_edit_intent_inner(None, intent, window, cx);
    }

    fn apply_edit_intent_inner(
        &mut self,
        target: Option<xiaomu_runtime::session::DocumentSelection>,
        intent: EditIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection_has_hidden_table_endpoint()
            || target.is_some_and(|selection| {
                let session = self.session.borrow();
                // Invalid targets belong to Runtime's typed rejection path.
                selection.validate(session.document()).is_ok()
                    && crate::table_capability::selection_has_hidden_table_endpoint(
                        session.document(),
                        selection,
                        &self.table_capability.borrow(),
                    )
            })
        {
            return;
        }
        if self.focused_child_composing(window, cx) {
            #[cfg(debug_assertions)]
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        self.desired_x = None;
        let outcome = self.apply_runtime_intent(target, &intent);
        match outcome {
            Ok(outcome) => {
                if outcome != xiaomu_runtime::session::SessionOutcome::NoChange {
                    self.epoch.set(self.epoch.get() + 1);
                }
                if outcome == xiaomu_runtime::session::SessionOutcome::DocumentChanged
                    || (target.is_some()
                        && outcome == xiaomu_runtime::session::SessionOutcome::SelectionChanged)
                {
                    // Materialize newly-created blocks or a changed range
                    // proxy before transferring native focus to the result.
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
                            DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => {
                                "caret at a structural boundary".to_owned()
                            }
                        }
                    };
                    eprintln!(
                        "xiaomu: structural command has no effect here [{where_am_i}]: {intent:?}"
                    );
                }
                cx.notify();
            }
            Err(error) => {
                eprintln!("xiaomu: intent rejected: {error}");
                self.emit_session_rejection(EditorRejectionStage::Intent, &error, cx);
            }
        }
    }

    fn apply_intent(&mut self, intent: EditIntent, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_edit_intent(intent, window, cx);
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

    /// Returns the decoded render source of one image block, when a fresh
    /// resolve has landed.
    #[must_use]
    pub fn image_render_source(&self, node: NodeId) -> Option<std::sync::Arc<gpui::Image>> {
        let document = self.session.borrow().document().clone();
        let node_data = document.node(node)?;
        let attrs = ImageAttrs::from_attrs(node_data.attrs()).ok()?;
        let source_key = match attrs.source() {
            ImageSource::AssetRef(value) => value.clone(),
            ImageSource::ExternalUrl(url) => url.clone(),
        };
        self.image_loads.render_source(node, &source_key)
    }

    /// Builds the label and background for one image placeholder.
    ///
    /// The state comes from the resolve cache; hosts without an asset
    /// service keep the neutral placeholder with the alt text.
    fn image_placeholder_presentation(&self, node: NodeId) -> (String, gpui::Rgba) {
        let document = self.session.borrow().document().clone();
        let Some(node_data) = document.node(node) else {
            return (String::new(), gpui::rgba(0xeeeeeeff));
        };
        let Ok(attrs) = ImageAttrs::from_attrs(node_data.attrs()) else {
            return ("invalid image attrs".to_owned(), gpui::rgba(0xf6d5d5ff));
        };
        let alt = attrs.alt().to_owned();
        let state = self.image_load_state(node);
        match state {
            Some(ImageLoadState::Loading) => (format!("加载中：{alt}"), gpui::rgba(0xe8eef7ff)),
            Some(ImageLoadState::Resolved { .. }) => {
                (format!("已解析：{alt}"), gpui::rgba(0xe2f2e4ff))
            }
            Some(ImageLoadState::Failed(_)) => (format!("加载失败：{alt}"), gpui::rgba(0xf6d5d5ff)),
            None => (alt, gpui::rgba(0xeeeeeeff)),
        }
    }

    /// Selects an atomic block as a whole node (plain click on its rule).
    pub(crate) fn select_atomic_block(
        &mut self,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.route_atomic_selection(node, window, cx) {
            return;
        }
        let outcome = self.session.borrow_mut().set_atomic_selection(node);
        match outcome {
            Ok(_) => {
                // Canonical node selection alone does not own native keyboard
                // actions. Also reclaim focus when a repeated click returns
                // NoChange after the host or another control took focus away.
                self.route_focus(window, cx);
                cx.notify();
            }
            Err(error) => eprintln!("xiaomu: selection rejected: {error}"),
        }
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
                // Node-selection anchors do not extend into text ranges in
                // this slice.
                DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => None,
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
        let selection = self.session.borrow().selection();
        if let Some(node) = selection.as_node_selection() {
            if let Some((anchor, input)) = &self.range_input
                && *anchor == node
            {
                input.read(cx).request_caret_scroll();
            }
            return;
        }
        let node = match selection.focus() {
            DocumentPosition::Inline(point) => point.node_id(),
            DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => return,
        };
        if let Some((_, view)) = self.children.iter().find(|(id, _)| *id == node) {
            view.read(cx).request_caret_scroll();
        }
    }

    // ---- rendering ----

    /// Syncs child entities to the current snapshot's block list, dropping
    /// views whose nodes no longer exist.
    fn sync_children(&mut self, cx: &mut Context<Self>) {
        self.focus_handle.get_or_insert_with(|| cx.focus_handle());
        self.sync_range_input(cx);
        let nodes: Vec<NodeId> = {
            let session = self.session.borrow();
            self.buildable_text_blocks(session.document())
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
                        ParagraphView::new_with_optional_history_clock(
                            session.clone(),
                            epoch.clone(),
                            registry.clone(),
                            node,
                            self.history_clock.clone(),
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
                view.attach_table_capability(self.table_capability.clone());
                view.set_code_block_presentation(self.code_block_presentation.clone());
            });
        }
        // Stale entries dropped with `pool`.
    }
}

impl Render for DocumentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.validate_column_resize(cx);
        // A host policy may commit into another node/All/cell range. Capture
        // ownership before syncing drops the old proxy, and transfer focus
        // only if that proxy owned it, including range-to-range transitions.
        // Measured tables may delete a focused cell while a host moves the
        // canonical caret elsewhere. Capture this pane's ownership before
        // sync_children drops that entity; post-measurement cannot recover it
        // from the new child list. An inactive pane never enters this branch.
        let restore_focus = ((self.table_capability.borrow().enabled()
            || self.selection_has_hidden_table_endpoint())
            && self.focused_child(window, cx).is_some())
            || (self.range_input_is_focused(window, cx)
                && self.range_input.as_ref().map(|(anchor, _)| *anchor)
                    != self.range_input_anchor());
        self.sync_children(cx);
        if restore_focus {
            self.route_focus(window, cx);
        }

        {
            let document = self.session.borrow().document().clone();
            sync_image_loads(&document, &self.image_loads, self.asset_service.as_ref());
        }

        if self.table_capability.borrow().enabled() {
            return self.render_measured_viewport(cx);
        }

        let root = self.session.borrow().document().root();

        // Each paint pass repopulates the registry; stale entries must go.
        self.registry.borrow_mut().clear();
        self.cell_registry.borrow_mut().clear();
        self.table_clips.borrow_mut().clear();

        let tree = self.render_block_tree(root, false, 0, 0, cx);

        self.render_scroll_tree(tree, cx)
    }
}
