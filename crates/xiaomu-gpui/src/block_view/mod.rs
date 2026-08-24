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

mod element;
mod input_handler;

use std::ops::Range;

use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ShapedLine, Subscription, Window, actions, div, prelude::*, px,
};

use xiaomu_core::document::{InlineContent, NodeId};
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_runtime::session::{CaretMove, DocumentSession, EditIntent};

use crate::input::composition::CompositionState;
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

    /// Ends the composition without committing, restoring the base
    /// selection through selection-only intents.
    fn cancel_composition(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.composition.take() else {
            return;
        };

        let selection = state.base_selection();
        let anchor = selection.anchor().offset();
        let focus = selection.focus().offset();
        self.apply_intent(
            EditIntent::PlaceCaret {
                offset: anchor,
                extend_selection: false,
            },
            cx,
        );
        self.apply_intent(
            EditIntent::PlaceCaret {
                offset: focus,
                extend_selection: true,
            },
            cx,
        );
    }

    /// Commits `text` over the composition's base range as exactly one
    /// transaction (one undo unit).
    fn commit_composition(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(state) = self.composition.take() else {
            return;
        };

        let range = state.base_range();
        let Ok(start) = self
            .inline()
            .map(|inline| inline.offset_at(range.start))
            .unwrap_or_else(|| Ok(TextOffset::ZERO))
        else {
            return;
        };
        let Ok(end) = self
            .inline()
            .map(|inline| inline.offset_at(range.end))
            .unwrap_or_else(|| Ok(TextOffset::ZERO))
        else {
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
        self.apply_intent(
            EditIntent::InsertText {
                text: text.to_owned(),
            },
            cx,
        );
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
        self.apply_intent_when_idle(intent, cx);
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

    /// Begins or updates the IME composition with a new preedit string.
    fn mark_text(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let canonical = self.canonical_text();

        if self.composition.is_none() {
            let (start, end) = match range_utf16 {
                Some(range) => (
                    utf16::utf8_offset(&canonical, range.start),
                    utf16::utf8_offset(&canonical, range.end),
                ),
                None => match self.ordered_range() {
                    Some(range) => (range.start().as_usize(), range.end().as_usize()),
                    None => return,
                },
            };

            let Some(inline) = self.inline() else {
                return;
            };
            let (Ok(start), Ok(end)) = (inline.offset_at(start), inline.offset_at(end)) else {
                return;
            };

            self.composition = Some(CompositionState::begin(
                self.session.selection(),
                start.as_usize()..end.as_usize(),
                new_text,
                new_selected_range,
            ));
        } else if let Some(state) = self.composition.as_mut() {
            *state = state.update(new_text, new_selected_range);
        }

        // The preedit is a view transient: without an explicit repaint the
        // marked text would stay invisible while the IME session continues.
        // This must fire on the first mark too, not only on updates.
        cx.notify();

        // Upstream GPUI Windows frame scheduling can starve `WM_PAINT` while
        // the IME message stream is dense (see function docs); force the
        // composition frame to compose and present synchronously.
        #[cfg(windows)]
        force_synchronous_redraw(window);
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

        let composition = self.composition.as_ref();
        let (base_start, base_end) = composition
            .map(|state| {
                let range = state.base_range();
                (range.start, range.end)
            })
            .unwrap_or((usize::MAX, usize::MAX));
        let preedit_len = composition.map_or(0, |state| state.preedit().len());

        // Maps a canonical byte offset outside the base range to the
        // displayed-text coordinate space.
        let mapped = |byte: usize| {
            if byte <= base_start {
                byte
            } else if byte >= base_end {
                byte - (base_end - base_start) + preedit_len
            } else {
                base_start
            }
        };

        let mut segments: Vec<DisplaySegment> = Vec::new();
        let mut cursor = 0usize;
        for run in inline.runs() {
            let run_start = cursor;
            let run_end = run_start + run.len_bytes();
            cursor = run_end;

            let marks = run.marks();
            let (bold, italic, underline, strike) = (
                marks.contains(xiaomu_core::document::MarkKind::Bold),
                marks.contains(xiaomu_core::document::MarkKind::Italic),
                marks.contains(xiaomu_core::document::MarkKind::Underline),
                marks.contains(xiaomu_core::document::MarkKind::Strike),
            );

            // Canonical pieces before, between, and after the base range.
            for (piece_start, piece_end) in [
                (run_start, run_end.min(base_start)),
                (run_start.max(base_end), run_end.max(base_end)),
            ] {
                if piece_start < piece_end {
                    segments.push(DisplaySegment {
                        start: mapped(piece_start),
                        text: run.text().as_str()[piece_start - run_start..piece_end - run_start]
                            .to_owned(),
                        bold,
                        italic,
                        underline,
                        strike,
                    });
                }
            }
        }

        if let Some(state) = composition {
            segments.push(DisplaySegment {
                start: base_start,
                text: state.preedit().to_owned(),
                bold: false,
                italic: false,
                underline: true,
                strike: false,
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

    /// Cancels the composition if one is active; used on focus loss.
    pub(crate) fn cancel_if_composing(&mut self, cx: &mut Context<Self>) {
        if self.is_composing() {
            self.cancel_composition(cx);
        }
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
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(ParagraphElement { view: cx.entity() })
    }
}

/// Forces a synchronous redraw of the window.
///
/// Workaround for upstream GPUI Windows frame scheduling (zed-industries/zed
/// issue #61469): `cx.notify()` only marks the view dirty, and the actual
/// frame runs from the low-priority `WM_PAINT`, which can be starved by the
/// dense keyboard / IME message stream while an IME composition is active —
/// frames are built (`prepaint`/`paint` run) but never presented until the
/// queue drains.
///
/// `RedrawWindow` with `RDW_INVALIDATE | RDW_UPDATENOW` sends `WM_PAINT`
/// synchronously, so the composition frame is composed and presented before
/// this call returns. This stays inside `xiaomu-gpui` (frontend boundary);
/// revisit when the upstream fix lands in a pinned gpui release.
#[cfg(windows)]
fn force_synchronous_redraw(window: &mut Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
    };

    if let Ok(handle) = window.window_handle()
        && let RawWindowHandle::Win32(win32) = handle.as_raw()
    {
        // SAFETY: `hwnd` is the live window handle handed out by the window
        // itself; `RedrawWindow` only requests a synchronous repaint of that
        // window and touches no other state.
        #[allow(unsafe_code)]
        unsafe {
            let _ = RedrawWindow(
                Some(HWND(win32.hwnd.get() as *mut core::ffi::c_void)),
                None,
                None,
                RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
            );
        }
    }
}
