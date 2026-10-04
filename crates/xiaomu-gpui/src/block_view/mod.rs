//! Single inline-bearing block view.
//!
//! A block view renders one inline node of the document owned by the shared
//! [`DocumentSession`]. It never mutates the document directly except along
//! the IME commit path; all other editing flows through runtime intents
//! applied by [`crate::document_view::DocumentView`]. Platform UTF-16
//! ranges are converted at this boundary (see [`crate::input::utf16`]).
//!
//! While IME composition is active, all input-handler queries answer against
//! a virtual projection (canonical prefix + preedit + suffix); see
//! [`crate::input::composition`].

mod display;
mod element;
#[cfg(test)]
mod hard_break_display_tests;
mod ime;
#[cfg(test)]
mod ime_atom_tests;
mod ime_geometry;
mod input_handler;
mod layout;
#[cfg(test)]
mod policy_input_tests;
mod projection;
mod scroll;
#[cfg(test)]
mod tests;
mod text_style;
#[cfg(test)]
mod text_style_input_tests;
#[cfg(test)]
mod unmark_tests;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, Pixels, ScrollHandle, Subscription, Window,
    actions, div, prelude::*,
};

use xiaomu_core::document::{InlineContent, NodeId, NodeKind};
use xiaomu_runtime::session::{DocumentPosition, DocumentSession, EditIntent};

use crate::code_presentation::CodeBlockPresentation;
use crate::document_view::cache_key::LayoutCacheKey;
use crate::inline_atom::InlineAtomRendererRegistry;
use crate::input::composition::CompositionState;
use layout::BlockTextLayout;

pub use element::ParagraphElement;

/// The session handle every block view of one editor shares.
pub type SharedSession = Rc<RefCell<DocumentSession>>;

/// Per-block paint geometry published to the document view each frame.
///
/// The document view consumes these entries for cross-block mouse hit
/// testing; entries are cleared at the start of every render pass.
pub type BlockBoundsRegistry = Rc<RefCell<Vec<(NodeId, Bounds<Pixels>)>>>;

actions!(
    xiaomu_gpui,
    [
        /// Delete one Unicode scalar backwards, or the whole selection.
        Backspace,
        /// Delete one Unicode scalar forwards, or the whole selection.
        Delete,
        /// Collapse to the previous scalar boundary or the selection start.
        Left,
        /// Collapse to the next scalar boundary or the selection end.
        Right,
        /// Extend the selection one scalar to the left.
        SelectLeft,
        /// Extend the selection one scalar to the right.
        SelectRight,
        /// Move up one visual line.
        Up,
        /// Move down one visual line.
        Down,
        /// Extend the selection one visual line up.
        SelectUp,
        /// Extend the selection one visual line down.
        SelectDown,
        /// Collapse to the current visual line start.
        Home,
        /// Collapse to the current visual line end.
        End,
        /// Extend the selection to the current visual line start.
        SelectHome,
        /// Extend the selection to the current visual line end.
        SelectEnd,
        /// Select the whole document.
        SelectAll,
        /// Split the focused block at the caret (Enter).
        Enter,
        /// Indent the focused list item (Tab).
        TabIndent,
        /// Outdent the focused list item (Shift-Tab).
        ShiftTabIndent,
        /// Copy the selected plain text to the clipboard.
        ClipboardCopy,
        /// Cut the selected plain text to the clipboard and delete it.
        ClipboardCut,
        /// Paste clipboard text at the caret / over the selection.
        ClipboardPaste,
        /// Toggle bold over the selection.
        ToggleBold,
        /// Toggle italic over the selection.
        ToggleItalic,
        /// Toggle inline-code over the selection.
        ToggleCode,
        /// Toggle underline over the selection.
        ToggleUnderline,
        /// Toggle strikethrough over the selection.
        ToggleStrike,
        /// Undo the newest history entry.
        Undo,
        /// Persist the current snapshot through the host adapter (Ctrl/Cmd-S).
        SaveDocument,
        /// Redo the newest undone entry.
        Redo,
    ]
);

use projection::{DisplaySegment, project_display_content};

/// How much of this block's text the document selection covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionProjection {
    /// Nothing to draw in this block.
    None,
    /// A collapsed caret at a displayed-text byte offset.
    Caret(usize),
    /// A highlighted span of displayed-text byte offsets.
    Highlight { start: usize, end: usize },
}

/// A block editor view rendering one inline node of the shared session.
pub struct ParagraphView {
    pub(super) session: SharedSession,
    node: NodeId,
    /// A frontend-only empty input surface anchored to an explicit range
    /// selection. Its offsets never identify canonical document content.
    range_input: bool,
    focus_handle: FocusHandle,
    pub(super) last_layout: Option<BlockTextLayout>,
    pub(super) last_bounds: Option<Bounds<Pixels>>,
    pub(super) cache_key: Option<LayoutCacheKey>,
    /// Render generation shared with the owning document view.
    pub(super) epoch: Rc<std::cell::Cell<u64>>,
    pub(crate) bounds_registry: BlockBoundsRegistry,
    pub(super) scroll_handle: Option<ScrollHandle>,
    pub(super) scroll_caret_pending: Cell<bool>,
    pub(super) atom_renderers: Rc<InlineAtomRendererRegistry>,
    code_block_presentation: Option<CodeBlockPresentation>,
    composition: Option<CompositionState>,
    /// Consume the remainder of an unsupported native composition without
    /// falling through to ordinary typing and deleting selected atoms.
    rejected_composition: bool,
    focus_out_subscription: Option<Subscription>,
}

impl ParagraphView {
    /// Creates a view rendering `node` from the shared session.
    ///
    /// The session's selection does not have to live inside `node`; views
    /// project the document selection onto their own text for painting.
    pub fn new(
        session: SharedSession,
        epoch: Rc<std::cell::Cell<u64>>,
        bounds_registry: BlockBoundsRegistry,
        node: NodeId,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            session,
            node,
            range_input: false,
            focus_handle,
            last_layout: None,
            last_bounds: None,
            cache_key: None,
            epoch,
            bounds_registry,
            scroll_handle: None,
            scroll_caret_pending: Cell::new(true),
            atom_renderers: Rc::new(InlineAtomRendererRegistry::new()),
            code_block_presentation: None,
            composition: None,
            rejected_composition: false,
            focus_out_subscription: None,
        }
    }

    /// Reuses native input/IME for a cell rectangle or complete root range.
    /// The identity is only a layout anchor; no document node is added.
    pub(crate) fn for_document_range(
        session: SharedSession,
        epoch: Rc<std::cell::Cell<u64>>,
        cell: NodeId,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::new(session, epoch, Rc::new(RefCell::new(Vec::new())), cell, cx);
        view.range_input = true;
        view
    }

    pub(crate) const fn is_range_input(&self) -> bool {
        self.range_input
    }

    /// Attaches the owning document viewport's scroll handle.
    pub(crate) fn attach_scroll_handle(&mut self, scroll_handle: ScrollHandle) {
        self.scroll_handle = Some(scroll_handle);
    }

    /// Attaches the owning document view's renderer registry.
    pub(crate) fn attach_atom_renderers(&mut self, renderers: Rc<InlineAtomRendererRegistry>) {
        self.atom_renderers = renderers;
    }

    /// Applies the owning editor's optional visual-only code presentation.
    pub(crate) fn set_code_block_presentation(
        &mut self,
        presentation: Option<CodeBlockPresentation>,
    ) {
        if self.code_block_presentation != presentation {
            self.code_block_presentation = presentation;
            self.last_layout = None;
            self.last_bounds = None;
            self.cache_key = None;
        }
    }

    pub(super) fn active_code_presentation(&self) -> Option<&CodeBlockPresentation> {
        if self.range_input {
            return None;
        }
        self.code_block_presentation.as_ref().filter(|_| {
            self.session
                .borrow()
                .document()
                .node(self.node)
                .is_some_and(|node| node.kind() == &NodeKind::CodeBlock)
        })
    }

    /// Returns the shared session rendered by this view.
    #[must_use]
    pub fn session(&self) -> &SharedSession {
        &self.session
    }

    /// Returns the inline node rendered by this view.
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns whether an IME composition is currently active.
    #[must_use]
    pub(crate) const fn is_composing(&self) -> bool {
        self.composition.is_some() || self.rejected_composition
    }

    /// Returns the virtual caret position while composing, in displayed-text
    /// byte offsets.
    #[must_use]
    pub(crate) fn composing_caret_byte(&self) -> Option<usize> {
        self.composition
            .as_ref()
            .and_then(|state| self.input_byte_to_layout(state.caret_virtual_byte()))
    }

    pub(crate) fn inline(&self) -> Option<InlineContent> {
        if self.range_input {
            let session = self.session.borrow();
            let selection = session.selection();
            let active = selection
                .active_cell_range()
                .is_some_and(|range| range.anchor() == self.node)
                || (selection.is_all(session.document()) && self.node == session.document().root());
            return active.then(InlineContent::empty);
        }
        self.session
            .borrow()
            .document()
            .node(self.node)?
            .content()
            .as_inline()
            .cloned()
    }

    /// Returns the canonical concatenated text of the inline node.
    pub(crate) fn canonical_text(&self) -> String {
        self.inline()
            .map(|inline| {
                inline
                    .runs()
                    .iter()
                    .map(|run| run.text().as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn apply_intent(&mut self, intent: EditIntent, cx: &mut Context<Self>) {
        self.apply_intent_with_selection(intent, None, cx);
    }

    fn apply_intent_with_selection(
        &mut self,
        intent: EditIntent,
        selection: Option<xiaomu_runtime::session::DocumentSelection>,
        cx: &mut Context<Self>,
    ) {
        let outcome = {
            let mut session = self.session.borrow_mut();
            match selection {
                Some(selection) => session.apply_intent_with_selection(selection, &intent),
                None => session.apply_intent(&intent),
            }
        };
        let applied = match outcome {
            Ok(_) => true,
            Err(error) => {
                eprintln!("xiaomu: intent rejected: {error}");
                false
            }
        };
        if applied {
            self.request_caret_scroll();
        }
        self.epoch.set(self.epoch.get() + 1);
        cx.notify();
    }

    /// Projects the document selection onto this block's displayed text.
    ///
    /// `order` lists the document's inline-bearing nodes in document order;
    /// blocks strictly between the selection endpoints are covered fully.
    #[must_use]
    pub(crate) fn projected_selection(&self, order: &[NodeId]) -> SelectionProjection {
        let session = self.session.borrow();
        let selection = session.selection();
        let document = session.document();
        if selection.is_all(document) && !self.is_range_input() {
            return SelectionProjection::Highlight {
                start: 0,
                end: self.canonical_text().len(),
            };
        }

        let endpoint = |position: DocumentPosition| match position {
            DocumentPosition::Inline(point) => {
                Some((point.node_id(), point.text_offset().as_usize()))
            }
            DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => None,
        };

        let Ok((head, tail)) = selection.ordered(document) else {
            return SelectionProjection::None;
        };
        let Some((head_node, head_byte)) = endpoint(head) else {
            return SelectionProjection::None;
        };
        let Some((tail_node, tail_byte)) = endpoint(tail) else {
            return SelectionProjection::None;
        };
        let Some(my_index) = order.iter().position(|id| *id == self.node) else {
            return SelectionProjection::None;
        };
        let Some(head_index) = order.iter().position(|id| *id == head_node) else {
            return SelectionProjection::None;
        };
        let Some(tail_index) = order.iter().position(|id| *id == tail_node) else {
            return SelectionProjection::None;
        };

        if my_index < head_index || my_index > tail_index {
            return SelectionProjection::None;
        }
        let text_len = self.canonical_text().len();
        let start = if head_node == self.node { head_byte } else { 0 };
        let end = if tail_node == self.node {
            tail_byte
        } else {
            text_len
        };

        if start >= end {
            SelectionProjection::Caret(start.min(text_len))
        } else {
            SelectionProjection::Highlight {
                start,
                end: end.min(text_len),
            }
        }
    }

    /// Ask the same Runtime resolver that models committed insertion. Never
    /// cache this: a collapsed SetMark can change StoredMarks without an epoch.
    fn preedit_marks(&self) -> xiaomu_core::document::MarkSet {
        let Some(state) = &self.composition else {
            return xiaomu_core::document::MarkSet::empty();
        };
        let Some(range) = self.inline().and_then(|inline| {
            xiaomu_core::text::TextRange::new(
                inline.offset_at(state.base_range().start).ok()?,
                inline.offset_at(state.base_range().end).ok()?,
            )
            .ok()
        }) else {
            return xiaomu_core::document::MarkSet::empty();
        };
        self.session
            .borrow()
            .effective_composition_marks(self.node, range)
            .unwrap_or_else(|_| xiaomu_core::document::MarkSet::empty())
    }

    /// Builds the displayed text plus its styled segments.
    ///
    /// Without an active composition this is the canonical content itself;
    /// while composing, the preedit is spliced in as an underlined segment
    /// and the replaced canonical span disappears until commit.
    #[must_use]
    pub(crate) fn display_content(&self) -> (String, Vec<DisplaySegment>) {
        let Some(inline) = self.inline() else {
            return (String::new(), Vec::new());
        };
        let marks = self.preedit_marks();
        let composition = self
            .composition
            .as_ref()
            .map(|state| (state.base_range(), state.preedit(), &marks));
        project_display_content(&inline, composition)
    }
}

impl Focusable for ParagraphView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ParagraphView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_out_subscription.is_none() {
            let entity = cx.entity().downgrade();
            self.focus_out_subscription =
                Some(
                    window.on_focus_out(&self.focus_handle, cx, move |_, _, cx| {
                        if let Some(view) = entity.upgrade() {
                            view.update(cx, |view, cx| view.cancel_if_composing(cx));
                        }
                    }),
                );
        }

        div()
            .key_context("XiaomuParagraph")
            .track_focus(&self.focus_handle(cx))
            .w_full()
            .cursor(gpui::CursorStyle::IBeam)
            .when_some(self.active_code_presentation(), |wrapper, presentation| {
                presentation.style_wrapper(wrapper)
            })
            .child(ParagraphElement { view: cx.entity() })
    }
}
