//! Single-paragraph block view.
//!
//! The view owns its [`DocumentSession`] and renders one inline node. All
//! editing flows through runtime intents; the view never mutates the
//! document directly. Platform UTF-16 ranges are converted at this boundary
//! (see [`crate::input::utf16`]).
//!
//! While IME composition is active, all input-handler queries answer against
//! a virtual projection (canonical prefix + preedit + suffix); see
//! [`crate::input::composition`].

mod actions;
mod element;
mod ime;
mod input_handler;
#[cfg(test)]
mod tests;

use std::ops::Range;

use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ShapedLine, Subscription, Window, actions, div, prelude::*, px,
};

use xiaomu_core::document::{InlineContent, NodeId};
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_runtime::session::{CaretMove, DocumentSession, EditIntent};

use crate::input::composition::CompositionState;

pub use element::ParagraphElement;

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
        /// Collapse to the paragraph's logical start.
        Home,
        /// Collapse to the paragraph's logical end.
        End,
        /// Extend the selection to the paragraph's logical start.
        SelectHome,
        /// Extend the selection to the paragraph's logical end.
        SelectEnd,
        /// Select the whole paragraph.
        SelectAll,
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
    composition: Option<(Range<usize>, &str)>,
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

/// A single-paragraph editor view over one [`DocumentSession`].
pub struct ParagraphView {
    session: DocumentSession,
    node: NodeId,
    focus_handle: FocusHandle,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    composition: Option<CompositionState>,
    focus_out_subscription: Option<Subscription>,
}

impl ParagraphView {
    /// Creates a view rendering `node` from the session.
    ///
    /// The session's selection must live inside `node`; P1 editing never
    /// leaves the single inline node.
    pub fn new(session: DocumentSession, node: NodeId, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            session,
            node,
            focus_handle,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            composition: None,
            focus_out_subscription: None,
        }
    }

    /// Returns the session rendered by this view.
    #[must_use]
    pub fn session(&self) -> &DocumentSession {
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

    fn inline(&self) -> Option<InlineContent> {
        self.session
            .document()
            .node(self.node)?
            .content()
            .as_inline()
            .cloned()
    }

    /// Returns the canonical concatenated text of the inline node.
    fn canonical_text(&self) -> String {
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

    /// Returns the end offset of the inline content.
    fn end_offset(&self) -> Option<TextOffset> {
        self.inline().map(|inline| {
            let len = inline.len_bytes();
            inline.offset_at(len).expect("end offset is always valid")
        })
    }

    fn apply_intent(&mut self, intent: EditIntent, cx: &mut Context<Self>) {
        if let Err(error) = self.session.apply_intent(&intent) {
            eprintln!("xiaomu: intent rejected: {error}");
        }
        cx.notify();
    }

    /// Editing actions are suspended while composing: every canonical edit
    /// would invalidate the captured base range. Platforms route those keys
    /// through the IME instead; anything that still arrives here is dropped.
    fn apply_intent_when_idle(&mut self, intent: EditIntent, cx: &mut Context<Self>) {
        if self.is_composing() {
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        self.apply_intent(intent, cx);
    }

    fn ordered_range(&self) -> Option<TextRange> {
        self.session
            .text_selection()
            .and_then(|selection| selection.ordered_range().ok())
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let collapsed = self
            .session
            .text_selection()
            .map(|selection| selection.is_collapsed())
            .unwrap_or(false);
        let intent = if collapsed {
            EditIntent::MoveCaret {
                caret_move: CaretMove::Backward,
                extend_selection: false,
            }
        } else if let Some(range) = self.ordered_range() {
            EditIntent::PlaceCaret {
                offset: range.start(),
                extend_selection: false,
            }
        } else {
            return;
        };
        self.apply_intent_when_idle(intent, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let collapsed = self
            .session
            .text_selection()
            .map(|selection| selection.is_collapsed())
            .unwrap_or(false);
        let intent = if collapsed {
            EditIntent::MoveCaret {
                caret_move: CaretMove::Forward,
                extend_selection: false,
            }
        } else if let Some(range) = self.ordered_range() {
            EditIntent::PlaceCaret {
                offset: range.end(),
                extend_selection: false,
            }
        } else {
            return;
        };
        self.apply_intent_when_idle(intent, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(
            EditIntent::MoveCaret {
                caret_move: CaretMove::Backward,
                extend_selection: true,
            },
            cx,
        );
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(
            EditIntent::MoveCaret {
                caret_move: CaretMove::Forward,
                extend_selection: true,
            },
            cx,
        );
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(
            EditIntent::MoveCaret {
                caret_move: CaretMove::ToStart,
                extend_selection: false,
            },
            cx,
        );
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(
            EditIntent::MoveCaret {
                caret_move: CaretMove::ToEnd,
                extend_selection: false,
            },
            cx,
        );
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(
            EditIntent::MoveCaret {
                caret_move: CaretMove::ToStart,
                extend_selection: true,
            },
            cx,
        );
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(
            EditIntent::MoveCaret {
                caret_move: CaretMove::ToEnd,
                extend_selection: true,
            },
            cx,
        );
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let Some(end) = self.end_offset() else {
            return;
        };
        self.apply_intent_when_idle(
            EditIntent::PlaceCaret {
                offset: TextOffset::ZERO,
                extend_selection: false,
            },
            cx,
        );
        self.apply_intent_when_idle(
            EditIntent::PlaceCaret {
                offset: end,
                extend_selection: true,
            },
            cx,
        );
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(EditIntent::Backspace, cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent_when_idle(EditIntent::Delete, cx);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_composing() {
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        if let Err(error) = self.session.undo() {
            eprintln!("xiaomu: undo rejected: {error}");
        }
        cx.notify();
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_composing() {
            eprintln!("xiaomu: editing action ignored during composition");
            return;
        }
        if let Err(error) = self.session.redo() {
            eprintln!("xiaomu: redo rejected: {error}");
        }
        cx.notify();
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        // Clicking cancels an ongoing composition before repositioning.
        self.cancel_composition(cx);
        if let Some(offset) = self.index_for_mouse_position(event.position) {
            self.apply_intent(
                EditIntent::PlaceCaret {
                    offset,
                    extend_selection: event.modifiers.shift,
                },
                cx,
            );
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting
            && let Some(offset) = self.index_for_mouse_position(event.position)
        {
            self.apply_intent(
                EditIntent::PlaceCaret {
                    offset,
                    extend_selection: true,
                },
                cx,
            );
        }
    }

    /// Maps a window-space point to the closest valid inline offset.
    fn index_for_mouse_position(&self, position: Point<Pixels>) -> Option<TextOffset> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let inline = self.inline()?;

        let text = self.canonical_text();
        let raw = if position.y < bounds.top() {
            0
        } else if position.y > bounds.bottom() {
            text.len()
        } else {
            layout.closest_index_for_x(position.x - bounds.left())
        };

        inline.offset_at(raw).ok()
    }

    /// Builds the displayed text plus its styled segments.
    ///
    /// Without an active composition this is the canonical content itself;
    /// while composing, the preedit is spliced in as an underlined segment
    /// and the replaced canonical span disappears until commit.
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
            .size_full()
            .bg(gpui::white())
            .p_4()
            .line_height(px(28.0))
            .text_size(px(20.0))
            .text_color(gpui::black())
            .cursor(gpui::CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::toggle_bold))
            .on_action(cx.listener(Self::toggle_italic))
            .on_action(cx.listener(Self::toggle_code))
            .on_action(cx.listener(Self::toggle_underline))
            .on_action(cx.listener(Self::toggle_strike))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(ParagraphElement { view: cx.entity() })
    }
}
