//! Decoded native pointer coordinates, not a substitute for native GUI acceptance.

use super::mounted::{active, down, edge, movement, open, up};
use super::*;
use gpui::TestAppContext;
use std::cell::Cell;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

// Match stock GPUI 0.2.2's XI2 coordinate decode, including its f32 precision.
fn x11_position(physical: f32, scale: f32) -> f32 {
    let fixed_point = (physical * 65_536.0) as i32;
    fixed_point as f32 / f32::from(u16::MAX) / scale
}

#[test]
fn column_resize_native_encoded_pointer_delta_projects_to_integer_width() {
    let start = x11_position(709.0, 1.0);
    let end = x11_position(749.0, 1.0);
    assert_eq!(end - start, 40.000_61);
    assert_eq!(drag_width(240, start, end, 25), Some(280));
    assert_eq!(
        drag_width(260, x11_position(749.0, 1.0), x11_position(719.0, 1.0), 25),
        Some(230)
    );
    for scale in [1.0, 1.25, 1.5, 2.0] {
        assert_eq!(
            drag_width(
                240,
                x11_position(709.0 * scale, scale),
                x11_position(749.0 * scale, scale),
                25
            ),
            Some(280)
        );
    }
}

#[test]
fn column_resize_pointer_projection_ties_bounds_and_nonfinite_coordinates() {
    for (initial, start, current, minimum, expected) in [
        (80, 100.5, 140.0, 25, Some(120)), // positive target 119.5 rounds up
        (80, 100.0, 99.5, 25, Some(80)),   // negative delta, target 79.5 rounds up
        (80, 100.0, 99.49, 25, Some(79)),
        (80, 100.25, 100.5, 25, Some(80)),
        (80, 100.0, -500.0, 25, Some(25)),
        (80, 100.0, 44.49, 25, Some(25)),
        (25, 100.0, 100.5, 25, Some(26)),
        (80, 100.0, 100.0, 100, Some(100)),
        (1_000_000, 0.0, -0.5, 25, Some(1_000_000)),
        (1_000_000, 0.0, 0.0, 25, Some(1_000_000)),
        (1_000_000, 0.0, 0.25, 25, None),
        (80, f32::MAX, f32::MAX, 25, Some(80)),
        (80, -f32::MAX, f32::MAX, 25, None),
        (80, f32::MAX, -f32::MAX, 25, Some(25)),
    ] {
        assert_eq!(drag_width(initial, start, current, minimum), expected);
    }
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(drag_width(80, invalid, 100.0, 25), None);
        assert_eq!(drag_width(80, 100.0, invalid, 25), None);
    }
}

#[gpui::test]
fn column_resize_native_projection_preview_commit_noop_and_history(cx: &mut TestAppContext) {
    for (physical_delta, expected) in [(40.0, 120), (-30.0, 50), (-500.0, 25), (0.1, 80)] {
        let f = fixture(false);
        let session = session(&f);
        let selection = session.borrow().selection();
        let handle = open(
            session.clone(),
            false,
            Rc::new(Cell::new(true)),
            Default::default(),
            cx,
        );
        let count = Rc::new(Cell::new(0));
        let commits = count.clone();
        handle
            .update(cx, |view, _, cx| {
                view.set_table_column_resize(Some(TableColumnResize::new(
                    config(),
                    |_, _| true,
                    move |intent, view, window, cx| {
                        commits.set(commits.get() + 1);
                        let transaction = {
                            let session = view.session.borrow();
                            let document = session.document();
                            let mut transaction = Transaction::new(TransactionOrigin::UserInput);
                            for placement in document.table_grid(intent.table).unwrap().origins() {
                                if placement.column() != intent.column {
                                    continue;
                                }
                                let attrs = NodeAttrs::new(
                                    [(
                                        "colwidth".into(),
                                        AttrValue::List(vec![AttrValue::Integer(i64::from(
                                            intent.width,
                                        ))]),
                                    )]
                                    .into(),
                                )
                                .unwrap();
                                if document.node(placement.cell()).unwrap().attrs() != &attrs {
                                    transaction =
                                        transaction.with_step(TransactionStep::SetNodeAttrs {
                                            node: placement.cell(),
                                            attrs,
                                        });
                                }
                            }
                            transaction
                        };
                        view.apply_edit_transaction(&transaction, window, cx)
                            .unwrap();
                    },
                )));
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        let edge = edge(handle, cx);
        let start = point(px(x11_position(f32::from(edge.x), 1.0)), edge.y);
        let end = point(
            px(x11_position(f32::from(edge.x) + physical_delta, 1.0)),
            edge.y,
        );
        down(handle, start, cx);
        assert!(active(handle, cx));
        // A separate intermediate preview must not accumulate into the next delta.
        movement(handle, point(start.x + px(10.25), start.y), true, cx);
        assert!(active(handle, cx));
        movement(handle, end, true, cx);
        assert!(active(handle, cx));
        handle
            .update(cx, |view, _, _| {
                let state = view.column_resize.drag.borrow();
                let drag = state.as_ref().unwrap();
                assert_eq!(drag.intent.initial_width, 80);
                assert_eq!(drag.intent.width, expected);
                assert_eq!(
                    view.cell_registry.borrow()[0].1.size.width,
                    px(expected as f32)
                );
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), f.document.store());
        assert_eq!(
            session.borrow().document().revision(),
            f.document.revision()
        );
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
        assert_eq!(count.get(), 0);
        up(handle, end, cx);
        assert!(!active(handle, cx));
        up(handle, end, cx);
        movement(handle, end, false, cx);
        assert_eq!(count.get(), 1);
        assert_eq!(session.borrow().selection(), selection);
        if expected == 80 {
            assert_eq!(session.borrow().document().store(), f.document.store());
            assert_eq!(
                session.borrow().document().revision(),
                f.document.revision()
            );
            assert_eq!(session.borrow().history_depths(), (0, 0));
        } else {
            assert_eq!(session.borrow().history_depths(), (1, 0));
            let resized = session.borrow().document().clone();
            session.borrow_mut().undo().unwrap();
            assert_eq!(session.borrow().document().store(), f.document.store());
            session.borrow_mut().redo().unwrap();
            assert_eq!(session.borrow().document().store(), resized.store());
            assert_eq!(session.borrow().history_depths(), (1, 0));
        }
    }
}
