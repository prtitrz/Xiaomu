//! Actual virtual-window coverage of the temporary table presentation boundary.
use std::{cell::RefCell, rc::Rc};

use gpui::{
    AppContext as _, Bounds, Context, Entity, EntityInputHandler, Focusable as _, Pixels,
    TestAppContext, VisualTestContext, Window, WindowHandle, div, point, prelude::*, px, size,
};
use xiaomu_core::document::{
    AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection, DocumentSession, EditIntent};

use super::{DocumentView, navigation, table_guard, visual_navigation::NavStep};
use crate::{block_view::SharedSession, editor::bind_default_editor_keys};

struct Fixture {
    document: XiaomuDocument,
    before: NodeId,
    after: NodeId,
    table: NodeId,
    cells: Vec<NodeId>,
    blocks: Vec<NodeId>,
    atom: NodeId,
}

fn paragraph(b: &mut NodeStoreBuilder, text: &str) -> NodeId {
    b.insert(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        NodeContent::Inline(
            InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
        ),
    )
    .unwrap()
}

fn container(b: &mut NodeStoreBuilder, kind: NodeKind, children: Vec<NodeId>) -> NodeId {
    b.insert(kind, NodeAttrs::empty(), NodeContent::children(children))
        .unwrap()
}

fn fixture(span: Option<&str>) -> Fixture {
    let mut b = NodeStoreBuilder::new();
    let before = paragraph(&mut b, "before");
    let after = paragraph(&mut b, "after");
    let atom = b
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let mut cells = Vec::new();
    let mut blocks = Vec::new();
    for index in 0..if span.is_some() { 3 } else { 4 } {
        let block = paragraph(&mut b, &format!("cell{index}"));
        blocks.push(block);
        let mut attrs = NodeAttrs::empty();
        if index == 0
            && let Some(span) = span
        {
            attrs = NodeAttrs::new([(span.to_owned(), AttrValue::Integer(2))].into()).unwrap();
        }
        cells.push(
            b.insert(
                if index == 0 {
                    NodeKind::TableHeader
                } else {
                    NodeKind::TableCell
                },
                attrs,
                NodeContent::children(if index == 0 {
                    vec![block, atom]
                } else {
                    vec![block]
                }),
            )
            .unwrap(),
        );
    }
    let split = if span == Some("colspan") { 1 } else { 2 };
    let first = container(&mut b, NodeKind::TableRow, cells[..split].to_vec());
    let second = container(&mut b, NodeKind::TableRow, cells[split..].to_vec());
    let table = container(&mut b, NodeKind::Table, vec![first, second]);
    let root = container(&mut b, NodeKind::Document, vec![before, table, after]);
    Fixture {
        document: XiaomuDocument::new(root, b.finish()).unwrap(),
        before,
        after,
        table,
        cells,
        blocks,
        atom,
    }
}

fn session(f: &Fixture, selection: DocumentSelection) -> SharedSession {
    Rc::new(RefCell::new(
        DocumentSession::new(f.document.clone(), selection).unwrap(),
    ))
}

fn open(cx: &mut TestAppContext, session: SharedSession) -> WindowHandle<DocumentView> {
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| DocumentView::new(session))
        })
        .unwrap()
    });
    handle
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
}

fn caret(f: &Fixture, node: NodeId, offset: usize) -> DocumentSelection {
    let inline = f
        .document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    DocumentSelection::collapsed(InlinePoint::new(
        node,
        inline.offset_at(offset).unwrap(),
        0,
        CursorAffinity::Before,
    ))
}

fn step(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>, key: &str) {
    cx.simulate_keystrokes(handle.into(), key);
    cx.background_executor.run_until_parked();
}

fn placeholder(
    cx: &mut TestAppContext,
    handle: WindowHandle<DocumentView>,
    table: NodeId,
) -> Bounds<Pixels> {
    VisualTestContext::from_window(handle.into(), cx)
        .debug_bounds(Box::leak(
            format!("unsupported-spanning-table-{table:?}").into_boxed_str(),
        ))
        .expect("a spanning table must paint its explicit placeholder")
}

#[gpui::test]
fn unit_header_uses_cell_ancestry_selection_and_ordinary_caret_geometry(cx: &mut TestAppContext) {
    let f = fixture(None);
    let s = session(&f, caret(&f, f.blocks[0], 0));
    let handle = open(cx, s.clone());
    handle
        .update(cx, |view, window, cx| {
            assert_eq!(
                navigation::table_cell_ancestor(&f.document, f.blocks[0]),
                Some(f.cells[0])
            );
            assert_eq!(
                navigation::table_cell_ancestor(&f.document, f.cells[0]),
                Some(f.cells[0])
            );
            assert!(!view.selection_has_hidden_table_endpoint());
            assert_eq!(view.cell_registry.borrow().len(), 4);
            let bounds = view.block_bounds(f.blocks[0]).unwrap();
            assert!(bounds.size.width > px(0.0));
            assert_eq!(
                view.vertical_neighbor(f.blocks[0], bounds.center().x, true),
                Some(f.blocks[2])
            );
            view.install_cell_range(f.cells[0], f.cells[1], window, cx);
            let range = s.borrow().selection().active_cell_range().unwrap();
            assert_eq!(
                navigation::cell_range_rect(&f.document, range),
                Some(f.cells[..2].to_vec())
            );
            assert!(view.range_input_is_focused(window, cx));
            assert!(view.navigate_cell_range(&NavStep::Down, true, window, cx));
            assert_eq!(
                s.borrow().selection().active_cell_range().unwrap().focus(),
                f.cells[3]
            );
            view.place(InlinePoint::at_start_of(f.blocks[0]), window, cx);
        })
        .unwrap();
    step(cx, handle, "right");
    assert_eq!(s.borrow().selection(), caret(&f, f.blocks[0], 1));
    step(cx, handle, "tab");
    assert_eq!(
        s.borrow().selection().focus(),
        DocumentPosition::Inline(InlinePoint::at_start_of(f.blocks[1]))
    );
    assert_eq!(s.borrow().document().store(), f.document.store());
    assert_eq!(s.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn ragged_span_rows_paint_placeholder_without_cell_registry_or_hidden_inputs(
    cx: &mut TestAppContext,
) {
    for span in ["rowspan", "colspan"] {
        let f = fixture(Some(span));
        let initial = caret(&f, f.before, 0);
        let s = session(&f, initial);
        let handle = open(cx, s.clone());
        let bounds = placeholder(cx, handle, f.table);
        handle
            .update(cx, |view, _, _| {
                assert!(view.cell_registry.borrow().is_empty());
                assert_eq!(
                    view.children.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                    [f.before, f.after]
                );
                assert!(
                    f.blocks
                        .iter()
                        .all(|node| view.block_bounds(*node).is_none())
                );
                assert!(view.range_input.is_none());
                let visible = table_guard::rendered_nav_units(&f.document);
                assert!(navigation::unit_index(&visible, f.atom).is_none());
                assert!(navigation::unit_index(&visible, f.blocks[0]).is_none());
                // Canonical helpers retain their original exhaustive semantics.
                assert!(
                    navigation::unit_index(&navigation::nav_units(&f.document), f.atom).is_some()
                );
                assert!(
                    navigation::block_index(&navigation::text_blocks(&f.document), f.blocks[0])
                        .is_some()
                );
            })
            .unwrap();
        VisualTestContext::from_window(handle.into(), cx)
            .simulate_click(bounds.center(), Default::default());
        assert_eq!(s.borrow().selection(), initial);
        assert_eq!(s.borrow().document().store(), f.document.store());
        assert_eq!(s.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn restored_hidden_caret_consumes_arrows_tab_and_editing_without_an_input_surface(
    cx: &mut TestAppContext,
) {
    let f = fixture(Some("rowspan"));
    let initial = caret(&f, f.blocks[0], 0);
    let s = session(&f, initial);
    let handle = open(cx, s.clone());
    placeholder(cx, handle, f.table);
    handle
        .update(cx, |view, window, cx| {
            assert!(view.focus_handle.as_ref().unwrap().is_focused(window));
            assert!(view.focused_child(window, cx).is_none());
            assert!(view.vertical_neighbor(f.blocks[0], px(0.0), true).is_none());
            view.apply_edit_intent(
                EditIntent::PasteText {
                    text: "hidden edit".into(),
                },
                window,
                cx,
            );
        })
        .unwrap();
    for key in [
        "right",
        "left",
        "up",
        "down",
        "shift-right",
        "shift-left",
        "shift-up",
        "shift-down",
        "home",
        "end",
        "tab",
        "shift-tab",
        "enter",
        "backspace",
    ] {
        step(cx, handle, key);
        assert_eq!(s.borrow().selection(), initial, "{key}");
        assert_eq!(s.borrow().document().store(), f.document.store(), "{key}");
    }
    cx.simulate_input(handle.into(), "invisible");
    assert_eq!(s.borrow().selection(), initial);
    assert_eq!(s.borrow().document().store(), f.document.store());
    assert_eq!(s.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn horizontal_fallback_skips_placeholder_descendants_in_both_directions(cx: &mut TestAppContext) {
    let f = fixture(Some("colspan"));
    let s = session(&f, caret(&f, f.before, 6));
    let handle = open(cx, s.clone());
    step(cx, handle, "right");
    assert_eq!(s.borrow().selection(), caret(&f, f.after, 0));
    step(cx, handle, "left");
    assert_eq!(s.borrow().selection(), caret(&f, f.before, 6));
    step(cx, handle, "shift-right");
    assert_eq!(
        s.borrow().selection().focus(),
        caret(&f, f.after, 0).focus()
    );
    assert_eq!(s.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn span_cell_hit_range_entry_and_restored_range_all_fail_closed(cx: &mut TestAppContext) {
    let f = fixture(Some("rowspan"));
    let initial = caret(&f, f.before, 0);
    let s = session(&f, initial);
    let handle = open(cx, s.clone());
    handle
        .update(cx, |view, window, cx| {
            // A previous frame's cell bounds must not revive a hidden target.
            let stale = Bounds::new(point(px(0.0), px(0.0)), size(px(100.0), px(100.0)));
            view.cell_registry.borrow_mut().push((f.cells[0], stale));
            assert_eq!(view.cell_at_position(stale.center()), None);
            view.begin_cell_range(f.cells[0], false, window, cx);
            assert!(view.cell_drag_anchor.is_none());
            view.install_cell_range(f.cells[0], f.cells[2], window, cx);
            assert_eq!(s.borrow().selection(), initial);
            assert!(view.range_input.is_none());
        })
        .unwrap();
    // The public selection shape may be restored with a parked caret outside
    // the table. Cell identities, not only park, must suppress its input proxy.
    let restored = DocumentSelection::cell_range(f.cells[0], f.cells[2], initial.focus());
    s.borrow_mut().set_document_selection(restored).unwrap();
    handle
        .update(cx, |view, window, cx| {
            view.focus_selection(window, cx);
            assert!(view.selection_has_hidden_table_endpoint());
            assert!(view.range_input.is_none());
            assert!(view.focus_handle.as_ref().unwrap().is_focused(window));
            for direction in [NavStep::Left, NavStep::Right, NavStep::Up, NavStep::Down] {
                assert!(view.navigate_cell_range(&direction, true, window, cx));
                assert!(view.navigate_cell_range(&direction, false, window, cx));
            }
            assert!(
                navigation::cell_range_rect(&f.document, restored.active_cell_range().unwrap())
                    .is_none()
            );
        })
        .unwrap();
    step(cx, handle, "escape");
    assert_eq!(s.borrow().selection(), restored);
    assert_eq!(s.borrow().document().store(), f.document.store());
    assert_eq!(s.borrow().history_depths(), (0, 0));
}

struct Panes {
    hidden: Entity<DocumentView>,
    ordinary: Entity<DocumentView>,
}
impl Render for Panes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .child(div().flex_1().min_w_0().h_full().child(self.hidden.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.ordinary.clone()),
            )
    }
}

#[gpui::test]
fn background_span_placeholder_does_not_steal_other_pane_focus_or_input(cx: &mut TestAppContext) {
    let hidden = fixture(Some("rowspan"));
    let ordinary = fixture(None);
    let hidden_selection = caret(&hidden, hidden.blocks[0], 0);
    let a = session(&hidden, hidden_selection);
    let b = session(&ordinary, caret(&ordinary, ordinary.blocks[0], 0));
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let hidden = cx.new(|_| DocumentView::new(a.clone()));
            let ordinary = cx.new(|_| DocumentView::new(b.clone()));
            cx.new(|_| Panes { hidden, ordinary })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |panes, window, cx| {
            window.activate_window();
            panes
                .ordinary
                .update(cx, |view, cx| view.focus_selection(window, cx));
            panes.hidden.update(cx, |_, cx| cx.notify());
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |panes, window, cx| {
            assert!(panes.ordinary.read(cx).focused_child(window, cx).is_some());
            assert!(panes.hidden.read(cx).focused_child(window, cx).is_none());
            assert_eq!(panes.hidden.read(cx).scroll_handle.offset().y, px(0.0));
        })
        .unwrap();
    cx.simulate_input(handle.into(), "visible");
    assert_eq!(a.borrow().document().store(), hidden.document.store());
    assert_eq!(a.borrow().selection(), hidden_selection);
    assert_eq!(a.borrow().history_depths(), (0, 0));
    assert_eq!(b.borrow().history_depths(), (1, 0));
}

#[gpui::test]
fn host_hidden_selection_revokes_retained_block_and_range_native_handlers(cx: &mut TestAppContext) {
    for range_proxy in [false, true] {
        let f = fixture(Some("rowspan"));
        let s = session(&f, caret(&f, f.before, 0));
        let handle = open(cx, s.clone());
        let old = handle
            .update(cx, |view, window, cx| {
                if range_proxy {
                    view.select_node(f.before, window, cx).unwrap();
                }
                let input = view.focused_child(window, cx).unwrap();
                input.update(cx, |input, cx| {
                    input.replace_and_mark_text_in_range(None, "nihao", Some(5..5), window, cx);
                    assert!(input.is_composing());
                });
                input
            })
            .unwrap();
        let hidden = caret(&f, f.blocks[0], 0);
        s.borrow_mut().set_document_selection(hidden).unwrap();
        // Host restores state before the next paint; only the former owner
        // should transfer its native focus to the inert DocumentView surface.
        handle.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |view, window, cx| {
                assert!(view.range_input.is_none());
                assert!(view.focus_handle.as_ref().unwrap().is_focused(window));
                old.update(cx, |input, cx| {
                    assert!(!input.focus_handle(cx).is_focused(window));
                    input.replace_and_mark_text_in_range(None, "late preedit", None, window, cx);
                    input.replace_text_in_range(None, "late commit", window, cx);
                    input.replace_text_in_range(Some(0..0), "late range", window, cx);
                    input.unmark_text(window, cx);
                    assert!(!input.is_composing());
                    assert!(input.selected_text_range(false, window, cx).is_none());
                    assert_eq!(input.marked_text_range(window, cx), None);
                    assert_eq!(input.text_for_range(0..0, &mut None, window, cx), None);
                });
            })
            .unwrap();
        cx.simulate_input(handle.into(), "late platform input");
        assert_eq!(s.borrow().selection(), hidden);
        assert_eq!(s.borrow().document().store(), f.document.store());
        assert_eq!(s.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn undo_restoring_hidden_endpoint_revokes_native_text_input(cx: &mut TestAppContext) {
    let f = fixture(Some("colspan"));
    let initial = caret(&f, f.blocks[0], 0);
    let s = session(&f, initial);
    // Headless Runtime still edits legal span text. GPUI's temporary layout
    // boundary must not turn into a canonical model restriction.
    s.borrow_mut()
        .apply_intent(&EditIntent::InsertText {
            text: "headless".into(),
        })
        .unwrap();
    assert_eq!(s.borrow().history_depths(), (1, 0));
    s.borrow_mut()
        .set_document_selection(caret(&f, f.before, 0))
        .unwrap();
    let handle = open(cx, s.clone());
    let old = handle
        .update(cx, |view, window, cx| {
            view.focused_child(window, cx).unwrap()
        })
        .unwrap();
    step(cx, handle, "ctrl-z");
    assert_eq!(s.borrow().selection(), initial);
    assert_eq!(s.borrow().document().store(), f.document.store());
    handle
        .update(cx, |view, window, cx| {
            assert!(view.focus_handle.as_ref().unwrap().is_focused(window));
            assert!(view.focused_child(window, cx).is_none());
            old.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "undo preedit", None, window, cx);
                input.unmark_text(window, cx);
                input.replace_text_in_range(None, "undo commit", window, cx);
                assert!(!input.is_composing());
            });
        })
        .unwrap();
    cx.simulate_input(handle.into(), "blocked");
    assert_eq!(s.borrow().document().store(), f.document.store());
    assert_eq!(s.borrow().selection(), initial);
    assert_eq!(s.borrow().history_depths(), (0, 1));
}

#[gpui::test]
fn retained_cell_handlers_cannot_edit_visible_selection_after_table_becomes_spanning(
    cx: &mut TestAppContext,
) {
    // Retain each native surface whose own identity is inside the table.
    for surface in [
        "paragraph",
        "cell range",
        "inner node range",
        "whole table range",
    ] {
        let f = fixture(None);
        let s = session(&f, caret(&f, f.blocks[0], 0));
        let handle = open(cx, s.clone());
        let old = handle
            .update(cx, |view, window, cx| {
                match surface {
                    "cell range" => view.install_cell_range(f.cells[0], f.cells[0], window, cx),
                    "inner node range" => {
                        view.select_node(f.blocks[0], window, cx).unwrap();
                    }
                    "whole table range" => {
                        view.select_node(f.table, window, cx).unwrap();
                    }
                    _ => {}
                }
                let input = view.focused_child(window, cx).unwrap();
                input.update(cx, |input, cx| {
                    input.replace_and_mark_text_in_range(None, "before merge", None, window, cx);
                    assert!(input.is_composing());
                });
                input
            })
            .unwrap();
        // Host changes the canonical table atomically, keeping the first
        // cell identity but covering the removed second cell's logical slot.
        let merge = Transaction::new(TransactionOrigin::System)
            .with_step(TransactionStep::SetNodeAttrs {
                node: f.cells[0],
                attrs: NodeAttrs::new([("colspan".into(), AttrValue::Integer(2))].into()).unwrap(),
            })
            .with_step(TransactionStep::RemoveNode { node: f.cells[1] });
        s.borrow_mut().apply(&merge).unwrap();
        let visible = caret(&f, f.before, 0);
        s.borrow_mut().set_document_selection(visible).unwrap();
        let merged = s.borrow().document().clone();
        let history = s.borrow().history_depths();
        handle
            .update(cx, |view, window, cx| {
                view.focus_selection(window, cx);
                assert!(!view.selection_has_hidden_table_endpoint());
                assert!(view.focused_child(window, cx).is_some());
                old.update(cx, |input, cx| {
                    assert!(!input.focus_handle(cx).is_focused(window));
                    // Selection is visible, so only the handler's now-hidden own
                    // paragraph/cell anchor prevents these from editing "before".
                    input.replace_and_mark_text_in_range(None, "late preedit", None, window, cx);
                    input.replace_text_in_range(None, "late result", window, cx);
                    input.replace_text_in_range(Some(0..0), "late explicit range", window, cx);
                    input.unmark_text(window, cx);
                    assert!(!input.is_composing());
                    assert!(input.selected_text_range(false, window, cx).is_none());
                    assert!(input.marked_text_range(window, cx).is_none());
                });
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        placeholder(cx, handle, f.table);
        assert_eq!(s.borrow().selection(), visible, "{surface}");
        assert_eq!(s.borrow().document().store(), merged.store(), "{surface}");
        assert_eq!(s.borrow().history_depths(), history, "{surface}");
        cx.simulate_input(handle.into(), "visible input");
        assert_ne!(s.borrow().document().store(), merged.store(), "{surface}");
        assert!(
            matches!(s.borrow().selection().focus(), DocumentPosition::Inline(point) if point.node_id() == f.before)
        );
    }
}

#[gpui::test]
fn visible_whole_table_and_all_proxies_keep_native_input_eligibility(cx: &mut TestAppContext) {
    let f = fixture(Some("colspan"));
    let s = session(&f, caret(&f, f.before, 0));
    let handle = open(cx, s.clone());
    for selection in [
        DocumentSelection::node(&f.document, f.table).unwrap(),
        DocumentSelection::all(&f.document),
    ] {
        s.borrow_mut().set_document_selection(selection).unwrap();
        handle
            .update(cx, |view, window, cx| {
                view.focus_selection(window, cx);
                assert!(!view.selection_has_hidden_table_endpoint());
                let proxy = view.range_input.as_ref().unwrap().1.clone();
                proxy.update(cx, |input, cx| {
                    assert!(input.focus_handle(cx).is_focused(window));
                    assert!(input.selected_text_range(false, window, cx).is_some());
                    input.replace_and_mark_text_in_range(None, "visible preedit", None, window, cx);
                    assert!(input.marked_text_range(window, cx).is_some());
                    input.replace_and_mark_text_in_range(None, "", None, window, cx);
                    assert!(!input.is_composing());
                });
            })
            .unwrap();
    }
    assert_eq!(s.borrow().document().store(), f.document.store());
    assert_eq!(s.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn atomic_host_target_cannot_bypass_current_or_target_hidden_table_guard(cx: &mut TestAppContext) {
    let f = fixture(Some("rowspan"));
    let visible = caret(&f, f.before, 0);
    let hidden = caret(&f, f.blocks[0], 0);
    for (original, target) in [(visible, hidden), (hidden, visible)] {
        let s = session(&f, original);
        let handle = open(cx, s.clone());
        handle
            .update(cx, |view, window, cx| {
                let epoch = view.epoch.get();
                view.apply_edit_intent_with_selection(
                    target,
                    EditIntent::InsertText {
                        text: "blocked".into(),
                    },
                    window,
                    cx,
                );
                assert_eq!(view.epoch.get(), epoch);
            })
            .unwrap();
        assert_eq!(s.borrow().document().store(), f.document.store());
        assert_eq!(s.borrow().document().revision(), f.document.revision());
        assert_eq!(s.borrow().selection(), original);
        assert_eq!(s.borrow().history_depths(), (0, 0));
    }
}
