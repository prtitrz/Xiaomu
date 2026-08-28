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

mod element;
mod ime;
mod input_handler;
mod layout;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, Pixels, Point, Subscription, Window, actions,
    div, point, prelude::*,
};

use xiaomu_core::document::{InlineContent, NodeId};
use xiaomu_runtime::session::{DocumentPosition, DocumentSession, EditIntent};

use crate::document_view::cache_key::LayoutCacheKey;
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

/// One styled span of the displayed (possibly virtual) text.
///
/// Byte offsets are relative to the displayed text so the element can shape
/// it without knowing about composition internals.
pub(crate) struct DisplaySegment {
    pub(super) start: usize,
    pub(super) text: String,
    pub(super) bold: bool,
    pub(super) italic: bool,
    pub(super) underline: bool,
    pub(super) strike: bool,
    pub(super) code: bool,
}

fn project_display_content(
    inline: &InlineContent,
    composition: Option<(std::ops::Range<usize>, &str)>,
) -> (String, Vec<DisplaySegment>) {
    let (base_start, base_end, preedit) = composition
        .as_ref()
        .map(|(range, text)| (range.start, range.end, *text))
        .unwrap_or((usize::MAX, usize::MAX, ""));
    let replaced_len = base_end.saturating_sub(base_start);

    let mut segments = Vec::new();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;

        let marks = run.marks();
        let style = (
            marks.contains(xiaomu_core::document::MarkKind::Bold),
            marks.contains(xiaomu_core::document::MarkKind::Italic),
            marks.contains(xiaomu_core::document::MarkKind::Underline),
            marks.contains(xiaomu_core::document::MarkKind::Strike),
            marks.contains(xiaomu_core::document::MarkKind::Code),
        );
        let mut push_piece = |start: usize, end: usize, display_start: usize| {
            if start < end {
                segments.push(DisplaySegment {
                    start: display_start,
                    text: run.text().as_str()[start - run_start..end - run_start].to_owned(),
                    bold: style.0,
                    italic: style.1,
                    underline: style.2,
                    strike: style.3,
                    code: style.4,
                });
            }
        };

        let prefix_end = run_end.min(base_start);
        push_piece(run_start, prefix_end, run_start);

        let suffix_start = run_start.max(base_end);
        let suffix_display_start = suffix_start.saturating_sub(replaced_len) + preedit.len();
        push_piece(suffix_start, run_end, suffix_display_start);
    }

    if let Some((range, text)) = composition {
        segments.push(DisplaySegment {
            start: range.start,
            text: text.to_owned(),
            bold: false,
            italic: false,
            underline: true,
            strike: false,
            code: false,
        });
    }

    segments.sort_by_key(|segment| segment.start);
    let mut text = String::new();
    for segment in &mut segments {
        segment.start = text.len();
        text.push_str(&segment.text);
    }

    (text, segments)
}

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
    focus_handle: FocusHandle,
    pub(super) last_layout: Option<BlockTextLayout>,
    pub(super) last_bounds: Option<Bounds<Pixels>>,
    pub(super) cache_key: Option<LayoutCacheKey>,
    /// Render generation shared with the owning document view.
    pub(super) epoch: Rc<std::cell::Cell<u64>>,
    pub(crate) bounds_registry: BlockBoundsRegistry,
    composition: Option<CompositionState>,
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
            focus_handle,
            last_layout: None,
            last_bounds: None,
            cache_key: None,
            epoch,
            bounds_registry,
            composition: None,
            focus_out_subscription: None,
        }
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
        self.composition.is_some()
    }

    /// Returns the virtual caret position while composing, in displayed-text
    /// byte offsets.
    #[must_use]
    pub(crate) fn composing_caret_byte(&self) -> Option<usize> {
        self.composition
            .as_ref()
            .map(CompositionState::caret_virtual_byte)
    }

    pub(crate) fn inline(&self) -> Option<InlineContent> {
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

    /// Ordered text range of the session selection when it is single-node.
    pub(crate) fn ordered_range(&self) -> Option<xiaomu_core::text::TextRange> {
        self.session
            .borrow()
            .text_selection()
            .and_then(|selection| selection.ordered_range().ok())
    }

    fn apply_intent(&mut self, intent: EditIntent, cx: &mut Context<Self>) {
        if let Err(error) = self.session.borrow_mut().apply_intent(&intent) {
            eprintln!("xiaomu: intent rejected: {error}");
        }
        self.epoch.set(self.epoch.get() + 1);
        cx.notify();
    }

    /// Maps a window-space point inside this block's last painted bounds to
    /// the closest raw byte index in its wrapped layout.
    pub(crate) fn hit_test_position(&self, position: Point<Pixels>) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        Some(layout.closest_index_for_position(point(
            position.x - bounds.left(),
            position.y - bounds.top(),
        )))
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

        let endpoint = |position: DocumentPosition| match position {
            DocumentPosition::Text(point) => Some((point.node_id(), point.offset().as_usize())),
            DocumentPosition::Gap(_) => None,
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

    /// Displayed-text byte offset of the selection focus when it lives in
    /// this block.
    #[must_use]
    pub(crate) fn focus_byte(&self) -> Option<usize> {
        let session = self.session.borrow();
        match session.selection().focus() {
            DocumentPosition::Text(point) if point.node_id() == self.node => {
                Some(point.offset().as_usize())
            }
            _ => None,
        }
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
        let composition = self
            .composition
            .as_ref()
            .map(|state| (state.base_range(), state.preedit()));
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
            .child(ParagraphElement { view: cx.entity() })
    }
}
