//! GPUI virtual-platform evidence, not a claim of native GUI acceptance.

use super::*;
use gpui::{
    AppContext as _, EntityInputHandler, MouseButton, TestAppContext, VisualTestContext,
    WindowHandle, point, px,
};
use std::cell::Cell;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

pub(super) fn open(
    session: SharedSession,
    enabled: bool,
    allowed: Rc<Cell<bool>>,
    commits: Rc<RefCell<Vec<TableColumnResizeIntent>>>,
    cx: &mut TestAppContext,
) -> WindowHandle<DocumentView> {
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| {
                let mut view = DocumentView::new(session);
                view.set_measured_table_layout(true);
                if enabled {
                    view.set_table_column_resize(Some(TableColumnResize::new(
                        config(),
                        move |_, _| allowed.get(),
                        move |intent, _, _, _| commits.borrow_mut().push(intent),
                    )));
                }
                view
            })
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

pub(super) fn edge(handle: WindowHandle<DocumentView>, cx: &mut TestAppContext) -> Point<Pixels> {
    handle
        .update(cx, |view, _, _| {
            let bounds = view.cell_registry.borrow()[0].1;
            point(bounds.right(), bounds.top() + px(20.0))
        })
        .unwrap()
}
pub(super) fn down(handle: WindowHandle<DocumentView>, p: Point<Pixels>, cx: &mut TestAppContext) {
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_down(
        p,
        MouseButton::Left,
        Default::default(),
    );
}
pub(super) fn movement(
    handle: WindowHandle<DocumentView>,
    p: Point<Pixels>,
    held: bool,
    cx: &mut TestAppContext,
) {
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_move(
        p,
        held.then_some(MouseButton::Left),
        Default::default(),
    );
}
pub(super) fn up(handle: WindowHandle<DocumentView>, p: Point<Pixels>, cx: &mut TestAppContext) {
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_up(
        p,
        MouseButton::Left,
        Default::default(),
    );
}
pub(super) fn active(handle: WindowHandle<DocumentView>, cx: &mut TestAppContext) -> bool {
    handle
        .update(cx, |view, _, _| view.column_resize.drag.borrow().is_some())
        .unwrap()
}

#[gpui::test]
fn column_resize_default_off_leaves_regular_pointer_selection_unchanged(cx: &mut TestAppContext) {
    let f = fixture(false);
    let session = session(&f);
    let commits = Rc::new(RefCell::new(Vec::new()));
    let handle = open(
        session.clone(),
        false,
        Rc::new(Cell::new(true)),
        commits.clone(),
        cx,
    );
    handle
        .update(cx, |view, _, _| {
            assert!(view.column_resize.measurements.borrow().is_empty())
        })
        .unwrap();
    let p = edge(handle, cx);
    down(handle, p, cx);
    movement(handle, p + point(px(50.0), px(0.0)), true, cx);
    up(handle, p, cx);
    assert!(!active(handle, cx));
    assert!(commits.borrow().is_empty());
    assert_eq!(session.borrow().document().store(), f.document.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn column_resize_reflows_real_children_without_session_mutation_then_commits_once(
    cx: &mut TestAppContext,
) {
    let f = fixture(false);
    let session = session(&f);
    let commits = Rc::new(RefCell::new(Vec::new()));
    let selection = session.borrow().selection();
    let handle = open(
        session.clone(),
        true,
        Rc::new(Cell::new(true)),
        commits.clone(),
        cx,
    );
    let p = edge(handle, cx);
    let before = handle
        .update(cx, |view, _, _| view.block_bounds(f.blocks[0]).unwrap())
        .unwrap();
    down(handle, p, cx);
    assert!(active(handle, cx));
    let moved = p - point(px(55.0), px(0.0));
    movement(handle, moved, true, cx);
    handle
        .update(cx, |view, window, cx| {
            let bounds = view.block_bounds(f.blocks[0]).unwrap();
            assert_eq!(view.cell_registry.borrow()[0].1.size.width, px(25.0));
            assert_eq!(bounds.size.width, px(1.0));
            assert!(
                bounds.size.height > before.size.height,
                "actual text wraps at transient width"
            );
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == f.blocks[0])
                .unwrap()
                .1
                .clone();
            child.update(cx, |child, cx| {
                assert_eq!(
                    child
                        .bounds_for_range(0..0, bounds, window, cx)
                        .unwrap()
                        .origin,
                    bounds.origin
                )
            });
        })
        .unwrap();
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().document().store(), f.document.store());
    assert_eq!(
        session.borrow().document().revision(),
        f.document.revision()
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
    assert!(commits.borrow().is_empty());
    up(handle, moved, cx);
    assert!(!active(handle, cx));
    assert_eq!(commits.borrow().len(), 1);
    assert_eq!(commits.borrow()[0].width, 25);
    assert_eq!(commits.borrow()[0].initial_width, 80);
    up(handle, moved, cx);
    movement(handle, moved, false, cx);
    assert_eq!(commits.borrow().len(), 1);
    handle
        .update(cx, |view, _, _| {
            assert_eq!(view.cell_registry.borrow()[0].1.size.width, px(80.0))
        })
        .unwrap();
}

#[gpui::test]
fn column_resize_window_release_and_button_free_move_finalize_even_outside_editor(
    cx: &mut TestAppContext,
) {
    for release in [true, false] {
        let f = fixture(false);
        let session = session(&f);
        let commits = Rc::new(RefCell::new(Vec::new()));
        let handle = open(session, true, Rc::new(Cell::new(true)), commits.clone(), cx);
        let p = edge(handle, cx);
        down(handle, p, cx);
        let outside = p + point(px(50.0), px(-500.0));
        movement(handle, outside, true, cx);
        assert!(active(handle, cx), "leaving editor is not cancellation");
        if release {
            up(handle, outside, cx);
        } else {
            movement(handle, outside, false, cx);
        }
        assert!(!active(handle, cx));
        assert_eq!(commits.borrow().len(), 1);
        assert_eq!(commits.borrow()[0].width, 130);
    }
}

#[gpui::test]
fn column_resize_guard_rejection_stale_revision_and_composition_cancel_without_callback(
    cx: &mut TestAppContext,
) {
    for mode in 0..6 {
        let f = fixture(false);
        let session = session(&f);
        let commits = Rc::new(RefCell::new(Vec::new()));
        let allowed = Rc::new(Cell::new(true));
        let handle = open(session.clone(), true, allowed.clone(), commits.clone(), cx);
        let p = edge(handle, cx);
        down(handle, p, cx);
        assert!(active(handle, cx));
        match mode {
            0 => allowed.set(false),
            1 => {
                session
                    .borrow_mut()
                    .apply_intent(&xiaomu_runtime::session::EditIntent::InsertText {
                        text: "late".into(),
                    })
                    .unwrap();
            }
            2 => {
                handle
                    .update(cx, |view, window, cx| {
                        let child = view.children[0].1.clone();
                        child.update(cx, |child, cx| {
                            child.replace_and_mark_text_in_range(None, "preedit", None, window, cx)
                        });
                    })
                    .unwrap();
            }
            3 => {
                handle
                    .update(cx, |view, _, _| view.set_table_column_resize(None))
                    .unwrap();
            }
            4 => {
                handle
                    .update(cx, |view, _, _| view.set_measured_table_layout(false))
                    .unwrap();
            }
            _ => {
                handle
                    .update(cx, |view, _, _| view.session = super::session(&f))
                    .unwrap();
            }
        }
        let before = session.borrow().document().clone();
        movement(handle, p + point(px(40.0), px(0.0)), true, cx);
        up(handle, p, cx);
        assert!(!active(handle, cx));
        assert!(commits.borrow().is_empty(), "case {mode}");
        assert_eq!(session.borrow().document().store(), before.store());
    }
}

#[gpui::test]
fn column_resize_nested_table_uses_nearest_real_geometry(cx: &mut TestAppContext) {
    let (f, outer) = nested_fixture();
    let session = session(&f);
    let commits = Rc::new(RefCell::new(Vec::new()));
    let handle = open(
        session.clone(),
        true,
        Rc::new(Cell::new(true)),
        commits.clone(),
        cx,
    );
    let (p, outer_width) = handle
        .update(cx, |view, _, _| {
            let measurements = view.column_resize.measurements.borrow();
            let outer_measurement = measurements.iter().find(|m| m.table == outer).unwrap();
            let inner = measurements.iter().find(|m| m.table == f.table).unwrap();
            (
                inner.origin + point(px(80.0), px(20.0)),
                outer_measurement.geometry.width,
            )
        })
        .unwrap();
    down(handle, p, cx);
    movement(handle, p - point(px(55.0), px(0.0)), true, cx);
    handle
        .update(cx, |view, _, _| {
            let measurements = view.column_resize.measurements.borrow();
            assert_eq!(
                measurements
                    .iter()
                    .find(|m| m.table == outer)
                    .unwrap()
                    .geometry
                    .width,
                outer_width
            );
            assert_eq!(
                measurements
                    .iter()
                    .find(|m| m.table == f.table)
                    .unwrap()
                    .geometry
                    .column_edges,
                vec![0.0, 25.0, 145.0]
            );
            assert_eq!(
                view.column_resize
                    .drag
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .intent
                    .table,
                f.table
            );
            assert_eq!(view.block_bounds(f.blocks[0]).unwrap().size.width, px(1.0));
        })
        .unwrap();
    up(handle, p - point(px(55.0), px(0.0)), cx);
    assert_eq!(commits.borrow().len(), 1);
    assert_eq!(commits.borrow()[0].table, f.table);
    assert_eq!(session.borrow().document().store(), f.document.store());
}

#[gpui::test]
fn column_resize_no_motion_callback_and_one_host_transaction_have_exact_undo(
    cx: &mut TestAppContext,
) {
    let f = fixture(false);
    let session = session(&f);
    let commits = Rc::new(Cell::new(0));
    let handle = open(
        session.clone(),
        false,
        Rc::new(Cell::new(true)),
        Default::default(),
        cx,
    );
    let count = commits.clone();
    let cells = f.cells.clone();
    handle
        .update(cx, |view, _, cx| {
            view.set_table_column_resize(Some(TableColumnResize::new(
                config(),
                |_, _| true,
                move |intent, view, window, cx| {
                    assert!(view.column_resize.drag.borrow().is_none());
                    count.set(count.get() + 1);
                    let mut transaction = Transaction::new(TransactionOrigin::UserInput);
                    for cell in [cells[0], cells[2]] {
                        let attrs = NodeAttrs::new(
                            [(
                                "colwidth".into(),
                                AttrValue::List(vec![AttrValue::Integer(i64::from(intent.width))]),
                            )]
                            .into(),
                        )
                        .unwrap();
                        if view.session.borrow().document().node(cell).unwrap().attrs() != &attrs {
                            transaction = transaction
                                .with_step(TransactionStep::SetNodeAttrs { node: cell, attrs });
                        }
                    }
                    view.apply_edit_transaction(&transaction, window, cx)
                        .unwrap();
                },
            )));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let p = edge(handle, cx);
    down(handle, p, cx);
    up(handle, p, cx);
    assert_eq!(commits.get(), 1);
    assert_eq!(
        session.borrow().history_depths(),
        (0, 0),
        "explicit unchanged attrs are no-op"
    );
    down(handle, p, cx);
    movement(handle, p + point(px(50.0), px(0.0)), true, cx);
    up(handle, p + point(px(50.0), px(0.0)), cx);
    assert_eq!(commits.get(), 2);
    assert_eq!(session.borrow().history_depths(), (1, 0));
    session.borrow_mut().undo().unwrap();
    assert_eq!(session.borrow().document().store(), f.document.store());
    session.borrow_mut().redo().unwrap();
    assert_eq!(session.borrow().history_depths(), (1, 0));
}

#[gpui::test]
fn column_resize_nonfinite_pointer_and_same_revision_different_document_cancel(
    cx: &mut TestAppContext,
) {
    for nonfinite in [true, false] {
        let f = fixture(false);
        let session = session(&f);
        let commits = Rc::new(RefCell::new(Vec::new()));
        let handle = open(
            session.clone(),
            true,
            Rc::new(Cell::new(true)),
            commits.clone(),
            cx,
        );
        let p = edge(handle, cx);
        down(handle, p, cx);
        if nonfinite {
            movement(handle, p + point(px(f32::NAN), px(0.0)), true, cx);
        } else {
            // A fresh snapshot can reuse all node IDs and the initial revision.
            let mut other =
                DocumentSession::new(f.document.clone(), session.borrow().selection()).unwrap();
            other
                .apply_intent(&xiaomu_runtime::session::EditIntent::InsertText {
                    text: "replacement".into(),
                })
                .unwrap();
            let replacement =
                XiaomuDocument::new(other.document().root(), other.document().store().clone())
                    .unwrap();
            assert_eq!(replacement.root(), f.document.root());
            assert_eq!(replacement.revision(), f.document.revision());
            *session.borrow_mut() = DocumentSession::new(replacement, other.selection()).unwrap();
            movement(handle, p, true, cx);
        }
        up(handle, p, cx);
        assert!(!active(handle, cx));
        assert!(commits.borrow().is_empty());
    }
}
