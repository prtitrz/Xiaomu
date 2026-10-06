use super::*;
use crate::table_column_resize::{
    TableColumnResize, TableColumnResizeConfig, TableColumnResizeIntent,
};
use gpui::{MouseButton, Pixels, Point};

fn install_resize(
    handle: WindowHandle<DocumentView>,
    cx: &mut TestAppContext,
) -> Rc<RefCell<Vec<TableColumnResizeIntent>>> {
    let commits = Rc::new(RefCell::new(Vec::new()));
    let saved = commits.clone();
    handle
        .update(cx, |view, _, cx| {
            view.set_table_column_resize(Some(TableColumnResize::new(
                TableColumnResizeConfig {
                    handle_width: 5.0,
                    min_column_width: 25,
                    last_column_resizable: true,
                },
                |_, _| true,
                move |intent, _, _, _| saved.borrow_mut().push(intent),
            )));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    commits
}

fn inner_offset(handle: WindowHandle<DocumentView>, f: &Nested, cx: &mut TestAppContext) -> Pixels {
    handle
        .update(cx, |view, _, _| {
            view.column_resize
                .measurements
                .borrow()
                .iter()
                .find(|table| table.table == f.inner)
                .unwrap()
                .viewport
                .as_ref()
                .unwrap()
                .offset
                .x
        })
        .unwrap()
}

fn scroll_right(handle: WindowHandle<DocumentView>, f: &Nested, cx: &mut TestAppContext) {
    let bounds = handle
        .update(cx, |view, _, _| {
            view.block_bounds(f.inner_blocks[1]).unwrap()
        })
        .unwrap();
    wheel(
        handle,
        bounds.origin + point(px(5.0), px(5.0)),
        -90.0,
        0.0,
        cx,
    );
}

fn drag_start(
    handle: WindowHandle<DocumentView>,
    f: &Nested,
    cx: &mut TestAppContext,
) -> Point<Pixels> {
    let p = handle
        .update(cx, |view, _, _| {
            let bounds = view
                .cell_registry
                .borrow()
                .iter()
                .find(|(id, _)| *id == f.inner_cells[1])
                .unwrap()
                .1;
            point(bounds.right() - px(1.0), bounds.top() + px(20.0))
        })
        .unwrap();
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_down(
        p,
        MouseButton::Left,
        Default::default(),
    );
    p
}

#[gpui::test]
fn measured_scrolled_resize_grow_shrink_grow_uses_original_offset_without_drift(
    cx: &mut TestAppContext,
) {
    let f = nested();
    let session = session(&f.fixture, f.inner_blocks[1]);
    let handle = open(session.clone(), cx);
    let commits = install_resize(handle, cx);
    scroll_right(handle, &f, cx);
    assert_eq!(inner_offset(handle, &f, cx), px(-54.0));
    let original = handle
        .update(cx, |view, _, _| {
            view.block_bounds(f.inner_blocks[1]).unwrap()
        })
        .unwrap();
    let p = drag_start(handle, &f, cx);
    for (delta, width, offset) in [(20.0, 150, -54.0), (-105.0, 25, 0.0), (20.0, 150, -54.0)] {
        VisualTestContext::from_window(handle.into(), cx).simulate_mouse_move(
            p + point(px(delta), px(0.0)),
            Some(MouseButton::Left),
            Default::default(),
        );
        handle
            .update(cx, |view, _, cx| {
                assert!(view.validate_column_resize(cx));
                let measured = view.column_resize.measurements.borrow();
                let inner = measured
                    .iter()
                    .find(|table| table.table == f.inner)
                    .unwrap();
                assert_eq!(
                    inner.geometry.column_edges[2] - inner.geometry.column_edges[1],
                    width as f32
                );
                assert_eq!(inner.viewport.as_ref().unwrap().offset.x, px(offset));
                let block = view.block_bounds(f.inner_blocks[1]).unwrap();
                if width == 25 {
                    assert!(
                        block.size.height > original.size.height,
                        "real children wrap and increase row height"
                    );
                }
            })
            .unwrap();
        assert_eq!(
            session.borrow().document().store(),
            f.fixture.document.store()
        );
        assert_eq!(session.borrow().history_depths(), (0, 0));
        assert!(commits.borrow().is_empty());
    }
    let end = p + point(px(20.0), px(0.0));
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_up(
        end,
        MouseButton::Left,
        Default::default(),
    );
    assert_eq!(commits.borrow().len(), 1);
    assert_eq!(
        (
            commits.borrow()[0].table,
            commits.borrow()[0].column,
            commits.borrow()[0].width
        ),
        (f.inner, 1, 150)
    );
    assert_eq!(
        session.borrow().document().store(),
        f.fixture.document.store()
    );
}

#[gpui::test]
fn measured_table_scroll_moves_native_caret_bounds_and_text_hits_together(cx: &mut TestAppContext) {
    let f = nested();
    let session = session(&f.fixture, f.inner_blocks[1]);
    let selection = session.borrow().selection();
    let handle = open(session.clone(), cx);
    let before = handle
        .update(cx, |view, window, cx| {
            let bounds = view.block_bounds(f.inner_blocks[1]).unwrap();
            view.focused_child(window, cx)
                .unwrap()
                .update(cx, |child, cx| {
                    child.bounds_for_range(0..0, bounds, window, cx).unwrap()
                })
        })
        .unwrap();
    scroll_right(handle, &f, cx);
    let caret = handle
        .update(cx, |view, window, cx| {
            let bounds = view.block_bounds(f.inner_blocks[1]).unwrap();
            let after = view
                .focused_child(window, cx)
                .unwrap()
                .update(cx, |child, cx| {
                    child.bounds_for_range(0..0, bounds, window, cx).unwrap()
                });
            assert_eq!(after.origin, before.origin - point(px(54.0), px(0.0)));
            after.origin + point(px(1.0), px(1.0))
        })
        .unwrap();
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(
        session.borrow().document().store(),
        f.fixture.document.store()
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_down(
        caret,
        MouseButton::Left,
        Default::default(),
    );
    assert_eq!(
        session.borrow().selection().focus(),
        xiaomu_runtime::session::DocumentPosition::Inline(InlinePoint::at_start_of(
            f.inner_blocks[1]
        ))
    );
}

#[gpui::test]
fn measured_scrolled_resize_cancels_unrelated_offset_and_outer_movement(cx: &mut TestAppContext) {
    for outer in [false, true] {
        let f = nested();
        let session = session(&f.fixture, f.inner_blocks[1]);
        let handle = open(session.clone(), cx);
        let commits = install_resize(handle, cx);
        scroll_right(handle, &f, cx);
        let p = drag_start(handle, &f, cx);
        handle
            .update(cx, |view, _, cx| {
                if outer {
                    view.scroll_handle.set_offset(point(px(0.0), px(-5.0)));
                } else {
                    let measured = view.column_resize.measurements.borrow();
                    measured
                        .iter()
                        .find(|table| table.table == f.inner)
                        .unwrap()
                        .viewport
                        .as_ref()
                        .unwrap()
                        .handle
                        .set_offset(point(px(-20.0), px(0.0)));
                }
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        VisualTestContext::from_window(handle.into(), cx).simulate_mouse_up(
            p,
            MouseButton::Left,
            Default::default(),
        );
        assert!(commits.borrow().is_empty());
        if !outer {
            assert_eq!(inner_offset(handle, &f, cx), px(-20.0));
        }
        assert_eq!(
            session.borrow().document().store(),
            f.fixture.document.store()
        );
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn measured_scrolled_resize_uses_actual_rounded_fractional_viewport(cx: &mut TestAppContext) {
    let mut f = nested();
    let mut transaction = Transaction::new(TransactionOrigin::System);
    for cell in &f.fixture.cells {
        transaction = transaction.with_step(TransactionStep::SetNodeAttrs {
            node: *cell,
            attrs: NodeAttrs::empty(),
        });
    }
    f.fixture.document = transaction.apply(&f.fixture.document).unwrap();
    let session = session(&f.fixture, f.inner_blocks[1]);
    let handle = open(session.clone(), cx);
    cx.simulate_window_resize(handle.into(), gpui::size(px(533.5), px(480.0)));
    cx.background_executor.run_until_parked();
    let commits = install_resize(handle, cx);
    scroll_right(handle, &f, cx);
    let original = handle
        .update(cx, |view, _, _| {
            view.column_resize
                .measurements
                .borrow()
                .iter()
                .find(|table| table.table == f.inner)
                .unwrap()
                .viewport
                .clone()
                .unwrap()
        })
        .unwrap();
    assert_eq!(original.constraint, px(226.75));
    assert_ne!(
        original.width, original.constraint,
        "virtual platform rounds actual bounds to device pixels"
    );
    assert_eq!(original.offset.x, -original.maximum);
    let p = drag_start(handle, &f, cx);
    for (delta, expected) in [(-105.0, px(0.0)), (20.0, original.offset.x)] {
        VisualTestContext::from_window(handle.into(), cx).simulate_mouse_move(
            p + point(px(delta), px(0.0)),
            Some(MouseButton::Left),
            Default::default(),
        );
        assert_eq!(inner_offset(handle, &f, cx), expected);
    }
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_up(
        p + point(px(20.0), px(0.0)),
        MouseButton::Left,
        Default::default(),
    );
    assert_eq!(commits.borrow().len(), 1);
    assert_eq!(commits.borrow()[0].width, 150);
    assert_eq!(
        session.borrow().document().store(),
        f.fixture.document.store()
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn measured_nested_scroll_keeps_live_preedit_and_repositions_its_native_bounds(
    cx: &mut TestAppContext,
) {
    let f = nested();
    let session = session(&f.fixture, f.inner_blocks[1]);
    let selection = session.borrow().selection();
    let handle = open(session.clone(), cx);
    handle
        .update(cx, |view, window, cx| {
            view.focused_child(window, cx)
                .unwrap()
                .update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(Some(0..0), "ni", Some(2..2), window, cx)
                });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let before = handle
        .update(cx, |view, window, cx| {
            let bounds = view.block_bounds(f.inner_blocks[1]).unwrap();
            view.focused_child(window, cx)
                .unwrap()
                .update(cx, |child, cx| {
                    assert!(child.is_composing());
                    child.bounds_for_range(2..2, bounds, window, cx).unwrap()
                })
        })
        .unwrap();
    scroll_right(handle, &f, cx);
    handle
        .update(cx, |view, window, cx| {
            let bounds = view.block_bounds(f.inner_blocks[1]).unwrap();
            view.focused_child(window, cx)
                .unwrap()
                .update(cx, |child, cx| {
                    assert!(
                        child.is_composing(),
                        "wheel must not force unmark or commit"
                    );
                    let after = child.bounds_for_range(2..2, bounds, window, cx).unwrap();
                    assert_eq!(after.origin, before.origin - point(px(54.0), px(0.0)));
                });
        })
        .unwrap();
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(
        session.borrow().document().store(),
        f.fixture.document.store()
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
