//! Real GPUI paint/scroll/resize coverage of the post-layout request decision.
//! TestPlatform's update_ime_position is a no-op; these tests do not establish
//! native XIM candidate movement or fresh synchronous preedit-query geometry.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AppContext as _, Bounds, Context, Entity, EntityInputHandler, IntoElement, Pixels, Point,
    Render, ScrollHandle, TestAppContext, Window, WindowBounds, WindowHandle, WindowOptions, div,
    point, prelude::*, px, size,
};
use xiaomu_core::{
    document::{
        AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

use super::super::{ParagraphView, SharedSession};

const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";

struct Panes {
    inputs: [Entity<ParagraphView>; 2],
    visible: [bool; 2],
    scroll: ScrollHandle,
    origin: Point<Pixels>,
}

impl Render for Panes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().relative().size_full().child(
            div()
                .id("ime-coordinate-scroll")
                .absolute()
                .left(self.origin.x)
                .top(self.origin.y)
                .size_full()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children(
                    self.inputs
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| self.visible[*index])
                        .map(|(_, input)| div().w_full().flex_shrink_0().child(input.clone())),
                )
                .child(div().h(px(800.0)).flex_shrink_0()),
        )
    }
}

fn open(
    cx: &mut TestAppContext,
    text: &str,
    hidden_table: bool,
) -> (WindowHandle<Panes>, SharedSession, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let mut child = node;
    if hidden_table {
        // A retained paragraph in a legacy-hidden spanning cell still paints,
        // but must not reclaim the platform's candidate-position ownership.
        for (kind, attrs) in [
            (
                NodeKind::TableCell,
                NodeAttrs::new([("colspan".into(), AttrValue::Integer(2))].into()).unwrap(),
            ),
            (NodeKind::TableRow, NodeAttrs::empty()),
            (NodeKind::Table, NodeAttrs::empty()),
        ] {
            child = builder
                .insert(kind, attrs, NodeContent::children([child]))
                .unwrap();
        }
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([child]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            document,
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        )
        .unwrap(),
    ));
    let handle = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(440.0), px(240.0)),
                ))),
                ..Default::default()
            },
            |_, cx| {
                let inputs = std::array::from_fn(|_| {
                    cx.new(|cx| {
                        ParagraphView::new(
                            session.clone(),
                            Rc::new(Cell::new(0)),
                            Rc::new(RefCell::new(Vec::new())),
                            node,
                            cx,
                        )
                    })
                });
                cx.new(|_| Panes {
                    inputs,
                    visible: [true; 2],
                    scroll: ScrollHandle::new(),
                    origin: point(px(0.0), px(0.0)),
                })
            },
        )
        .unwrap()
    });
    handle
        .update(cx, |panes, window, cx| {
            window.activate_window();
            window.focus(&panes.inputs[0].read(cx).focus_handle);
        })
        .unwrap();
    repaint(handle, cx);
    (handle, session, node)
}

fn repaint(handle: WindowHandle<Panes>, cx: &mut TestAppContext) {
    handle.update(cx, |_, window, _| window.refresh()).unwrap();
    cx.background_executor.run_until_parked();
}

fn observed(
    handle: WindowHandle<Panes>,
    index: usize,
    cx: &mut TestAppContext,
) -> (usize, Option<Bounds<Pixels>>) {
    handle
        .update(cx, |panes, _, cx| {
            let state = &panes.inputs[index].read(cx).ime_coordinates;
            (state.notifications, state.bounds)
        })
        .unwrap()
}

fn current_bounds(handle: WindowHandle<Panes>, cx: &mut TestAppContext) -> Bounds<Pixels> {
    handle
        .update(cx, |panes, window, cx| {
            panes.inputs[0].update(cx, |view, cx| {
                let selected = view.selected_text_range(true, window, cx).unwrap();
                let head = if selected.reversed {
                    selected.range.start
                } else {
                    selected.range.end
                };
                let native = view
                    .bounds_for_range(head..head, view.last_bounds.unwrap(), window, cx)
                    .unwrap();
                assert_eq!(view.ime_coordinates.bounds, Some(native));
                native
            })
        })
        .unwrap()
}

fn select(session: &SharedSession, node: NodeId, anchor: usize, focus: usize) {
    let inline = session
        .borrow()
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .clone();
    let at = |byte| {
        InlinePoint::new(
            node,
            inline.offset_at(byte).unwrap(),
            0,
            CursorAffinity::Before,
        )
    };
    session
        .borrow_mut()
        .set_inline_selection(at(anchor), at(focus))
        .unwrap();
}

#[gpui::test]
fn initial_and_changed_selection_heads_notify_but_stable_paints_do_not(cx: &mut TestAppContext) {
    let (handle, session, node) = open(cx, TEXT, false);
    let first = current_bounds(handle, cx);
    assert_eq!(observed(handle, 0, cx).0, 1);
    assert_eq!(observed(handle, 1, cx), (0, None));
    for _ in 0..3 {
        repaint(handle, cx);
        assert_eq!(observed(handle, 0, cx), (1, Some(first)));
    }

    select(&session, node, 0, 20);
    repaint(handle, cx);
    let forward = current_bounds(handle, cx);
    assert_ne!(forward, first);
    assert_eq!(observed(handle, 0, cx).0, 2);

    // The same selected range now has its head at the start, as stock GPUI
    // selected_bounds does. Comparing the whole range would miss this change.
    select(&session, node, 20, 0);
    repaint(handle, cx);
    assert_eq!(current_bounds(handle, cx), first);
    assert_eq!(observed(handle, 0, cx).0, 3);
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx).0, 3);
    assert_eq!(observed(handle, 1, cx), (0, None));
}

#[gpui::test]
fn passive_scroll_translation_and_wrap_resize_publish_current_coordinates(cx: &mut TestAppContext) {
    let (handle, session, node) = open(cx, TEXT, false);
    select(&session, node, TEXT.len(), TEXT.len());
    repaint(handle, cx);
    let initial = current_bounds(handle, cx);
    let before = observed(handle, 0, cx).0;
    handle
        .update(cx, |panes, _, _| {
            panes.scroll.set_offset(point(px(0.0), px(-24.0)))
        })
        .unwrap();
    repaint(handle, cx);
    let scrolled = current_bounds(handle, cx);
    assert_eq!(scrolled.origin, initial.origin - point(px(0.0), px(24.0)));
    assert_eq!(observed(handle, 0, cx).0, before + 1);

    handle
        .update(cx, |panes, _, _| panes.origin = point(px(17.0), px(11.0)))
        .unwrap();
    repaint(handle, cx);
    let translated = current_bounds(handle, cx);
    assert_eq!(
        translated.origin,
        scrolled.origin + point(px(17.0), px(11.0))
    );
    assert_eq!(observed(handle, 0, cx).0, before + 2);

    cx.simulate_window_resize(handle.into(), size(px(190.0), px(240.0)));
    cx.background_executor.run_until_parked();
    let narrow = current_bounds(handle, cx);
    assert!(
        narrow.top() > translated.top(),
        "the caret must move to a later wrapped row"
    );
    assert_eq!(observed(handle, 0, cx).0, before + 3);
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx), (before + 3, Some(narrow)));

    cx.simulate_window_resize(handle.into(), size(px(440.0), px(240.0)));
    cx.background_executor.run_until_parked();
    assert_eq!(current_bounds(handle, cx), translated);
    assert_eq!(observed(handle, 0, cx).0, before + 4);
}

#[gpui::test]
fn unfocused_and_inactive_panes_clear_the_stamp_without_requesting_notifications(
    cx: &mut TestAppContext,
) {
    let (handle, session, node) = open(cx, TEXT, false);
    let before = observed(handle, 0, cx).0;
    handle
        .update(cx, |panes, window, cx| {
            window.focus(&panes.inputs[1].read(cx).focus_handle)
        })
        .unwrap();
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx), (before, None));
    assert_eq!(observed(handle, 1, cx).0, 1);
    select(&session, node, 10, 10);
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx), (before, None));
    assert_eq!(observed(handle, 1, cx).0, 2);

    handle
        .update(cx, |panes, window, cx| {
            window.focus(&panes.inputs[0].read(cx).focus_handle)
        })
        .unwrap();
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx).0, before + 1);
    assert_eq!(observed(handle, 1, cx), (2, None));

    // A focused handle alone is insufficient when another window is active.
    let (_other, _, _) = open(cx, "other window", false);
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx), (before + 1, None));
    select(&session, node, 30, 30);
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx), (before + 1, None));
    handle
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx).0, before + 2);
    current_bounds(handle, cx);
}

#[gpui::test]
fn focused_retained_hidden_table_handler_does_not_request_coordinates(cx: &mut TestAppContext) {
    let (handle, session, node) = open(cx, TEXT, true);
    handle
        .update(cx, |panes, window, cx| {
            let input = panes.inputs[0].read(cx);
            assert!(input.focus_handle.is_focused(window));
            assert!(window.is_window_active());
            assert!(input.input_is_hidden_by_table());
            assert!(
                input.last_layout.is_some(),
                "the retained child really painted"
            );
        })
        .unwrap();
    assert_eq!(observed(handle, 0, cx), (0, None));
    select(&session, node, 20, 20);
    repaint(handle, cx);
    cx.simulate_window_resize(handle.into(), size(px(190.0), px(240.0)));
    cx.background_executor.run_until_parked();
    assert_eq!(observed(handle, 0, cx), (0, None));
}

#[gpui::test]
fn late_native_queries_reject_the_previous_focus_owner_and_inactive_window(
    cx: &mut TestAppContext,
) {
    let (handle, _, _) = open(cx, TEXT, false);
    let before = observed(handle, 0, cx);
    handle
        .update(cx, |panes, window, cx| {
            window.focus(&panes.inputs[1].read(cx).focus_handle);
            // No draw or focus-out callback has run since the switch. This is the
            // old input handler that stock GPUI can still query before repaint.
            panes.inputs[0].update(cx, |view, cx| {
                assert!(!view.focus_handle.is_focused(window));
                assert!(view.last_layout.is_some());
                assert_eq!(view.ime_coordinates.bounds, before.1);
                assert_eq!(
                    view.bounds_for_range(0..0, view.last_bounds.unwrap(), window, cx),
                    None
                );
            });
            window.focus(&panes.inputs[0].read(cx).focus_handle);
        })
        .unwrap();
    assert_eq!(observed(handle, 0, cx), before);

    let checked = Rc::new(Cell::new(false));
    let deactivated = checked.clone();
    let _subscription = handle
        .update(cx, |_, window, cx| {
            cx.observe_window_activation(window, move |panes, window, cx| {
                if !window.is_window_active() {
                    // Activation observers run before the inactive frame draws.
                    // The focus handle still belongs to this mounted paragraph.
                    panes.inputs[0].update(cx, |view, cx| {
                        assert!(view.focus_handle.is_focused(window));
                        assert_eq!(view.ime_coordinates.bounds, before.1);
                        let late_bounds =
                            view.bounds_for_range(0..0, view.last_bounds.unwrap(), window, cx);
                        if cfg!(any(target_os = "linux", target_os = "freebsd")) {
                            assert_eq!(late_bounds, None);
                        } else {
                            // Preserve platforms whose activation callback can
                            // arrive after an immediate native IME query.
                            assert!(late_bounds.is_some());
                        }
                    });
                    deactivated.set(true);
                }
            })
        })
        .unwrap();
    let (_other, _, _) = open(cx, "other window", false);
    assert!(checked.get(), "the late inactive query must actually run");
    assert_eq!(observed(handle, 0, cx), (before.0, None));
}

#[gpui::test]
fn retained_unpainted_pane_refocus_notifies_again_at_the_same_coordinates(cx: &mut TestAppContext) {
    let (handle, _, _) = open(cx, TEXT, false);
    let original = current_bounds(handle, cx);
    let before = observed(handle, 0, cx).0;
    handle
        .update(cx, |panes, window, cx| {
            panes.inputs[0]
                .read(cx)
                .bounds_registry
                .borrow_mut()
                .clear();
            panes.visible[0] = false;
            window.focus(&panes.inputs[1].read(cx).focus_handle);
        })
        .unwrap();
    repaint(handle, cx);
    handle
        .update(cx, |panes, _, cx| {
            assert!(
                panes.inputs[0].read(cx).bounds_registry.borrow().is_empty(),
                "the retained pane must not paint while hidden"
            );
        })
        .unwrap();
    assert_eq!(observed(handle, 0, cx), (before, None));

    handle
        .update(cx, |panes, window, cx| {
            panes.visible[0] = true;
            window.focus(&panes.inputs[0].read(cx).focus_handle);
        })
        .unwrap();
    repaint(handle, cx);
    assert_eq!(current_bounds(handle, cx), original);
    assert_eq!(observed(handle, 0, cx), (before + 1, Some(original)));
    repaint(handle, cx);
    assert_eq!(observed(handle, 0, cx), (before + 1, Some(original)));
}

#[gpui::test]
fn growing_unicode_preedit_notifies_after_paint_without_canonical_edits(cx: &mut TestAppContext) {
    let (handle, session, _) = open(cx, "AZ", false);
    let before = session.borrow().document().clone();
    let selection = session.borrow().selection();
    let mut previous = current_bounds(handle, cx);
    let mut notifications = observed(handle, 0, cx).0;
    for preedit in ["你".to_owned(), "中文🙂 e\u{301} nihao ".repeat(12)] {
        let units = preedit.encode_utf16().count();
        handle
            .update(cx, |panes, window, cx| {
                panes.inputs[0].update(cx, |view, cx| {
                    view.replace_and_mark_text_in_range(
                        None,
                        &preedit,
                        Some(units..units),
                        window,
                        cx,
                    );
                });
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        let current = current_bounds(handle, cx);
        assert_ne!(current, previous);
        assert_eq!(observed(handle, 0, cx).0, notifications + 1);
        handle
            .update(cx, |panes, _, cx| {
                let view = panes.inputs[0].read(cx);
                let layout = view.last_layout.as_ref().unwrap();
                let byte = view.composing_caret_byte().unwrap();
                let position = layout
                    .position_for_index(byte)
                    .expect("new preedit caret has real painted geometry");
                assert_eq!(current.origin, view.last_bounds.unwrap().origin + position);
                let shaped: String = layout
                    .lines()
                    .iter()
                    .map(|line| line.text.as_ref())
                    .collect();
                assert_eq!(shaped, format!("{preedit}AZ"));
                assert!(view.is_composing());
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().document().revision(), before.revision());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
        repaint(handle, cx);
        notifications += 1;
        assert_eq!(observed(handle, 0, cx), (notifications, Some(current)));
        previous = current;
    }
    assert!(previous.top() > px(0.0), "the grown preedit must wrap");
}
