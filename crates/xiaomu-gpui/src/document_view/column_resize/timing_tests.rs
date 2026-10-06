//! Release ordering and strict reuse of an actual successful preview.
use super::mounted::{down, edge, movement, open};
use super::*;
use gpui::{EntityInputHandler, MouseDownEvent, TestAppContext};
use std::cell::Cell;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn install_commit(handle: gpui::WindowHandle<DocumentView>, cx: &mut TestAppContext) {
    handle
        .update(cx, |view, _, cx| {
            view.set_table_column_resize(Some(TableColumnResize::new(
                config(),
                |_, _| true,
                move |intent, view, window, cx| {
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
}

#[gpui::test]
fn column_resize_measured_release_then_immediate_native_text_keeps_both_history_units(
    cx: &mut TestAppContext,
) {
    let f = fixture(false);
    let session = session(&f);
    let handle = open(
        session.clone(),
        false,
        Rc::new(Cell::new(true)),
        Default::default(),
        cx,
    );
    install_commit(handle, cx);
    let p = edge(handle, cx);
    down(handle, p, cx);
    let moved = p + point(px(50.0), px(0.0));
    movement(handle, moved, true, cx);
    let resized = handle
        .update(cx, |view, window, cx| {
            view.end_column_resize(moved, window, cx);
            assert!(
                view.column_resize.drag.borrow().is_none(),
                "measured release commits synchronously"
            );
            assert_eq!(session.borrow().history_depths(), (1, 0));
            let resized = session.borrow().document().clone();
            let child = view
                .focused_child(window, cx)
                .expect("same measured input retains admission and focus");
            child.update(cx, |child, cx| {
                child.replace_text_in_range(None, "X", window, cx)
            });
            assert_eq!(
                session.borrow().history_depths(),
                (2, 0),
                "no repaint before native input"
            );
            resized
        })
        .unwrap();
    session.borrow_mut().undo().unwrap();
    assert_eq!(session.borrow().document().store(), resized.store());
    session.borrow_mut().undo().unwrap();
    assert_eq!(session.borrow().document().store(), f.document.store());
}

#[gpui::test]
fn column_resize_old_queued_callback_cannot_touch_a_replacement_gesture(cx: &mut TestAppContext) {
    let f = fixture(false);
    let commits = Rc::new(Cell::new(0));
    let count = commits.clone();
    let handle = open(
        session(&f),
        false,
        Rc::new(Cell::new(true)),
        Default::default(),
        cx,
    );
    handle
        .update(cx, |view, _, cx| {
            view.set_table_column_resize(Some(TableColumnResize::new(
                config(),
                |_, _| true,
                move |_, _, _, _| count.set(count.get() + 1),
            )));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let p = edge(handle, cx);
    down(handle, p, cx);
    handle
        .update(cx, |view, window, cx| {
            view.end_column_resize(p + point(px(40.0), px(0.0)), window, cx);
            let token = view
                .column_resize
                .drag
                .borrow()
                .as_ref()
                .unwrap()
                .token
                .clone();
            let capability = view.column_resize.capability.clone();
            view.set_table_column_resize(capability);
            assert!(view.begin_column_resize(
                &MouseDownEvent {
                    position: p,
                    ..Default::default()
                },
                window,
                cx
            ));
            let replacement = view
                .column_resize
                .drag
                .borrow()
                .as_ref()
                .unwrap()
                .token
                .clone();
            assert!(!Rc::ptr_eq(&token, &replacement));
            view.commit_measured_column_resize(&token, window, cx);
            assert!(Rc::ptr_eq(
                &replacement,
                &view.column_resize.drag.borrow().as_ref().unwrap().token
            ));
            assert_eq!(commits.get(), 0);
        })
        .unwrap();
}

#[test]
fn column_resize_admission_transfer_rejects_non_width_content_kind_and_attributes() {
    use super::super::admission::only_column_widths_changed;
    let f = fixture(false);
    let plan = TableLayoutPlan::from_document(&f.document, f.table, Default::default()).unwrap();
    let width_attrs = NodeAttrs::new(
        [(
            "colwidth".into(),
            AttrValue::List(vec![AttrValue::Integer(130)]),
        )]
        .into(),
    )
    .unwrap();
    let width =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeAttrs {
            node: f.cells[0],
            attrs: width_attrs.clone(),
        });
    let changed = width.apply(&f.document).unwrap();
    assert!(only_column_widths_changed(&f.document, &changed, &plan));
    let mut extra = width_attrs
        .iter()
        .map(|(k, v)| (k.to_owned(), v.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    extra.insert("backgroundColor".into(), AttrValue::String("red".into()));
    let changed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: f.cells[0],
            attrs: NodeAttrs::new(extra).unwrap(),
        })
        .apply(&f.document)
        .unwrap();
    assert!(!only_column_widths_changed(&f.document, &changed, &plan));
    let changed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeKind {
            node: f.cells[0],
            kind: NodeKind::TableHeader,
        })
        .apply(&f.document)
        .unwrap();
    assert!(!only_column_widths_changed(&f.document, &changed, &plan));
    let merged = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: f.cells[0],
            attrs: NodeAttrs::new(
                [
                    ("colspan".into(), AttrValue::Integer(2)),
                    (
                        "colwidth".into(),
                        AttrValue::List(vec![AttrValue::Integer(80), AttrValue::Integer(120)]),
                    ),
                ]
                .into(),
            )
            .unwrap(),
        })
        .with_step(TransactionStep::RemoveNode { node: f.cells[1] })
        .apply(&f.document)
        .unwrap();
    assert!(!only_column_widths_changed(&f.document, &merged, &plan));
    let editing = session(&f);
    editing
        .borrow_mut()
        .apply_intent(&xiaomu_runtime::session::EditIntent::InsertText {
            text: "changed".into(),
        })
        .unwrap();
    assert!(!only_column_widths_changed(
        &f.document,
        editing.borrow().document(),
        &plan
    ));
}

#[gpui::test]
fn column_resize_different_canonical_tracks_cannot_reuse_preview_admission(
    cx: &mut TestAppContext,
) {
    let f = fixture(false);
    let session = session(&f);
    let handle = open(
        session.clone(),
        false,
        Rc::new(Cell::new(true)),
        Default::default(),
        cx,
    );
    let cells = f.cells.clone();
    handle
        .update(cx, |view, _, cx| {
            view.set_table_column_resize(Some(TableColumnResize::new(
                config(),
                |_, _| true,
                move |intent, view, window, cx| {
                    let mut transaction = Transaction::new(TransactionOrigin::UserInput);
                    for cell in [cells[0], cells[2]] {
                        transaction = transaction.with_step(TransactionStep::SetNodeAttrs {
                            node: cell,
                            attrs: NodeAttrs::new(
                                [(
                                    "colwidth".into(),
                                    AttrValue::List(vec![AttrValue::Integer(
                                        i64::from(intent.width) + 1,
                                    )]),
                                )]
                                .into(),
                            )
                            .unwrap(),
                        });
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
    let moved = p + point(px(50.0), px(0.0));
    movement(handle, moved, true, cx);
    handle
        .update(cx, |view, window, cx| {
            view.end_column_resize(moved, window, cx);
            assert!(
                !view
                    .table_capability
                    .borrow()
                    .permits(session.borrow().document(), f.table)
            );
        })
        .unwrap();
}
