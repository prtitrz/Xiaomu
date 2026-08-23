//! Single-paragraph block view.
//!
//! The view owns its [`DocumentSession`] and renders one inline node. All
//! editing flows through runtime intents; the view never mutates the
//! document directly. Platform UTF-16 ranges are converted at this boundary
//! (see [`crate::input::utf16`]).

mod element;

use std::ops::Range;

use gpui::{
    App, Bounds, Context, EntityInputHandler, FocusHandle, Focusable, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine, UTF16Selection, Window, actions, div,
    prelude::*, px,
};

use xiaomu_core::document::{InlineContent, NodeId};
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_runtime::session::{CaretMove, DocumentSession, EditIntent};

use crate::input::utf16;

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
        /// Undo the newest history entry.
        Undo,
        /// Redo the newest undone entry.
        Redo,
    ]
);

/// A single-paragraph editor view over one [`DocumentSession`].
pub struct ParagraphView {
    session: DocumentSession,
    node: NodeId,
    focus_handle: FocusHandle,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
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

    fn inline(&self) -> Option<InlineContent> {
        self.session
            .document()
            .node(self.node)?
            .content()
            .as_inline()
            .cloned()
    }

    fn text(&self) -> String {
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

    fn ordered_range(&self) -> Option<TextRange> {
        self.session.selection().ordered_range().ok()
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let intent = if self.session.selection().is_collapsed() {
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
        self.apply_intent(intent, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let intent = if self.session.selection().is_collapsed() {
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
        self.apply_intent(intent, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(
            EditIntent::MoveCaret {
                caret_move: CaretMove::Backward,
                extend_selection: true,
            },
            cx,
        );
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(
            EditIntent::MoveCaret {
                caret_move: CaretMove::Forward,
                extend_selection: true,
            },
            cx,
        );
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(
            EditIntent::MoveCaret {
                caret_move: CaretMove::ToStart,
                extend_selection: false,
            },
            cx,
        );
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(
            EditIntent::MoveCaret {
                caret_move: CaretMove::ToEnd,
                extend_selection: false,
            },
            cx,
        );
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(
            EditIntent::MoveCaret {
                caret_move: CaretMove::ToStart,
                extend_selection: true,
            },
            cx,
        );
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(
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
        self.apply_intent(
            EditIntent::PlaceCaret {
                offset: TextOffset::ZERO,
                extend_selection: false,
            },
            cx,
        );
        self.apply_intent(
            EditIntent::PlaceCaret {
                offset: end,
                extend_selection: true,
            },
            cx,
        );
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::Backspace, cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_intent(EditIntent::Delete, cx);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.session.undo() {
            eprintln!("xiaomu: undo rejected: {error}");
        }
        cx.notify();
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.session.redo() {
            eprintln!("xiaomu: redo rejected: {error}");
        }
        cx.notify();
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
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

        let text = self.text();
        let raw = if position.y < bounds.top() {
            0
        } else if position.y > bounds.bottom() {
            text.len()
        } else {
            layout.closest_index_for_x(position.x - bounds.left())
        };

        inline.offset_at(raw).ok()
    }
}

impl Focusable for ParagraphView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ParagraphView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(ParagraphElement { view: cx.entity() })
    }
}

impl EntityInputHandler for ParagraphView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.text();
        let start = utf16::utf8_offset(&text, range_utf16.start);
        let end = utf16::utf8_offset(&text, range_utf16.end);
        adjusted_range.replace(utf16::utf16_offset(&text, start)..utf16::utf16_offset(&text, end));
        Some(text.get(start..end)?.to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.session.selection();
        let anchor = selection.anchor().offset().as_usize();
        let focus = selection.focus().offset().as_usize();
        let text = self.text();
        let (start, end) = (anchor.min(focus), anchor.max(focus));
        Some(UTF16Selection {
            range: utf16::utf16_offset(&text, start)..utf16::utf16_offset(&text, end),
            // The platform sees the focus (cursor) as the selection head.
            reversed: focus < anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        // IME composition state lands in P1.4; P1.3 has no marked text.
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(range_utf16) = replacement_range {
            // Select the explicit range with selection-only intents (no
            // history entries), then insert over it as one transaction.
            let full_text = self.text();
            let start = utf16::utf8_offset(&full_text, range_utf16.start);
            let end = utf16::utf8_offset(&full_text, range_utf16.end);
            let Some(inline) = self.inline() else {
                return;
            };
            let (Ok(start), Ok(end)) = (inline.offset_at(start), inline.offset_at(end)) else {
                return;
            };
            self.apply_intent(
                EditIntent::PlaceCaret {
                    offset: start,
                    extend_selection: false,
                },
                cx,
            );
            self.apply_intent(
                EditIntent::PlaceCaret {
                    offset: end,
                    extend_selection: true,
                },
                cx,
            );
        }
        self.apply_intent(
            EditIntent::InsertText {
                text: text.to_owned(),
            },
            cx,
        );
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // P1.3 stopgap: no composition state yet, so preedit text commits
        // immediately as plain input. Full marked-text semantics land in
        // P1.4 together with the composition state machine.
        self.replace_text_in_range(range_utf16, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let text = self.text();
        let start = utf16::utf8_offset(&text, range_utf16.start);
        let end = utf16::utf8_offset(&text, range_utf16.end);
        Some(Bounds::from_corners(
            gpui::point(
                element_bounds.left() + layout.x_for_index(start),
                element_bounds.top(),
            ),
            gpui::point(
                element_bounds.left() + layout.x_for_index(end),
                element_bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let text = self.text();
        let raw = if point.y < bounds.top() {
            0
        } else if point.y > bounds.bottom() {
            text.len()
        } else {
            layout.closest_index_for_x(point.x - bounds.left())
        };
        Some(utf16::utf16_offset(&text, raw))
    }
}
