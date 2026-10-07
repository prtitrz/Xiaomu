//! Read-only pending-state queries, including the deferred-commit boundary.

use super::mounted::{down, edge, movement, open, up};
use super::*;
use gpui::TestAppContext;
use std::cell::Cell;

#[test]
fn pending_column_resize_query_is_false_when_idle_or_merely_enabled() {
    let f = fixture(false);
    let mut view = DocumentView::new(session(&f));
    assert!(!view.has_pending_table_column_resize());
    view.set_measured_table_layout(true);
    view.set_table_column_resize(Some(TableColumnResize::new(
        config(),
        |_, _| panic!("a query must not run the guard"),
        |_, _, _, _| panic!("a query must not commit"),
    )));
    assert!(!view.has_pending_table_column_resize());
    assert!(!view.has_pending_table_column_resize());
}

#[gpui::test]
fn pending_column_resize_query_preserves_outer_and_nested_previews(cx: &mut TestAppContext) {
    for nested in [false, true] {
        let f = if nested {
            nested_fixture().0
        } else {
            fixture(false)
        };
        let session = session(&f);
        let selection = session.borrow().selection();
        let allowed = Rc::new(Cell::new(true));
        let commits = Rc::new(RefCell::new(Vec::new()));
        let handle = open(session.clone(), true, allowed.clone(), commits.clone(), cx);
        let p = handle
            .update(cx, |view, _, _| {
                let measurements = view.column_resize.measurements.borrow();
                let table = measurements.iter().find(|m| m.table == f.table).unwrap();
                table.origin + point(px(80.0), px(20.0))
            })
            .unwrap();
        movement(handle, p, false, cx);
        handle
            .update(cx, |view, _, _| {
                assert!(view.column_resize.hovered);
                assert!(!view.has_pending_table_column_resize());
            })
            .unwrap();
        down(handle, p, cx);
        movement(handle, p - point(px(55.0), px(0.0)), true, cx);
        handle
            .update(cx, |view, window, cx| {
                // A host preflight must observe even a now-invalid preview,
                // leaving validation/cancellation to the existing lifecycle.
                allowed.set(false);
                let other_focus = cx.focus_handle();
                window.focus(&other_focus);
                let view: &DocumentView = view;
                let drag = view.column_resize.drag.borrow();
                let drag = drag.as_ref().unwrap();
                assert_eq!(drag.intent.table, f.table);
                assert_eq!(drag.intent.width, 25);
                assert!(!drag.released);
                assert!(!drag.commit_queued);
                let viewport = drag.viewport.as_ref().unwrap();
                let offset = viewport.handle.offset();
                let bounds = view.block_bounds(f.blocks[0]).unwrap();
                // Holding these borrows detects hidden session access and
                // mutation of the live preview as well as repeated-query bugs.
                let session = session.borrow_mut();
                for _ in 0..3 {
                    assert!(view.has_pending_table_column_resize());
                    assert!(other_focus.is_focused(window));
                    assert_eq!(viewport.handle.offset(), offset);
                    assert_eq!(view.block_bounds(f.blocks[0]), Some(bounds));
                    assert_eq!(session.selection(), selection);
                    assert_eq!(session.document().store(), f.document.store());
                    assert_eq!(session.document().revision(), f.document.revision());
                    assert_eq!(session.history_depths(), (0, 0));
                    assert!(commits.borrow().is_empty());
                }
            })
            .unwrap();
        handle
            .update(cx, |view, _, _| {
                view.set_table_column_resize(None);
                assert!(!view.has_pending_table_column_resize());
                assert!(!view.has_pending_table_column_resize());
            })
            .unwrap();
        assert!(commits.borrow().is_empty());
    }
}

#[gpui::test]
fn pending_column_resize_query_includes_unmeasured_release_until_completion(
    cx: &mut TestAppContext,
) {
    let f = fixture(false);
    let commits = Rc::new(RefCell::new(Vec::new()));
    let handle = open(
        session(&f),
        true,
        Rc::new(Cell::new(true)),
        commits.clone(),
        cx,
    );
    let p = edge(handle, cx);
    down(handle, p, cx);
    handle
        .update(cx, |view, window, cx| {
            view.end_column_resize(p + point(px(40.0), px(0.0)), window, cx);
            let state = view.column_resize.drag.borrow();
            let drag = state.as_ref().unwrap();
            assert!(drag.released);
            assert!(!drag.commit_queued);
            assert!(view.has_pending_table_column_resize());
            assert!(view.has_pending_table_column_resize());
            assert!(commits.borrow().is_empty());
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| {
            assert!(!view.has_pending_table_column_resize());
        })
        .unwrap();
    assert_eq!(commits.borrow().len(), 1);
    up(handle, p, cx);
    assert_eq!(commits.borrow().len(), 1);
}

#[gpui::test]
fn pending_column_resize_query_includes_queued_commit_without_consuming_it(
    cx: &mut TestAppContext,
) {
    let f = fixture(false);
    let commits = Rc::new(RefCell::new(Vec::new()));
    let recorded = commits.clone();
    let handle = open(
        session(&f),
        false,
        Rc::new(Cell::new(true)),
        commits.clone(),
        cx,
    );
    handle
        .update(cx, |view, _, cx| {
            view.set_table_column_resize(Some(TableColumnResize::new(
                config(),
                |_, _| true,
                move |intent, view, _, _| {
                    assert!(!view.has_pending_table_column_resize());
                    recorded.borrow_mut().push(intent);
                },
            )));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let p = edge(handle, cx);
    down(handle, p, cx);
    movement(handle, p + point(px(40.0), px(0.0)), true, cx);
    handle
        .update(cx, |view, window, cx| {
            // Establish the state seen when a released preview has just
            // measured, then use the real measurement-completion queue path.
            view.column_resize
                .drag
                .borrow_mut()
                .as_mut()
                .unwrap()
                .released = true;
            view.finish_column_resize_measurement(window, cx);
            let state = view.column_resize.drag.borrow();
            let drag = state.as_ref().unwrap();
            assert!(drag.released);
            assert!(drag.commit_queued);
            for _ in 0..3 {
                assert!(view.has_pending_table_column_resize());
                assert!(commits.borrow().is_empty());
            }
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| {
            assert!(!view.has_pending_table_column_resize());
        })
        .unwrap();
    assert_eq!(commits.borrow().len(), 1);
    assert_eq!(commits.borrow()[0].width, 120);
}
