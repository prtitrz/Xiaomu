//! Child lifetime, native controls and reading behavior in read-only input mode.
use super::*;
use crate::table_column_resize::{TableColumnResize, TableColumnResizeConfig};
use gpui::{MouseButton, VisualTestContext, point, px};
use xiaomu_core::{
    document::AttrValue,
    transaction::{DocumentTemplate, Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::session::{SelectionUpdate, SessionOutcome};

fn table() -> (XiaomuDocument, Vec<NodeId>, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let mut blocks = Vec::new();
    let mut cells = Vec::new();
    for _ in 0..2 {
        let block = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("cell text", MarkSet::empty()).unwrap()])
                        .unwrap(),
                ),
            )
            .unwrap();
        blocks.push(block);
        cells.push(
            builder
                .insert(
                    NodeKind::TableCell,
                    NodeAttrs::new(
                        [(
                            "colwidth".into(),
                            AttrValue::List(vec![AttrValue::Integer(120)]),
                        )]
                        .into(),
                    )
                    .unwrap(),
                    NodeContent::children([block]),
                )
                .unwrap(),
        );
    }
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells.clone()),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        blocks,
        cells,
    )
}

#[gpui::test]
fn read_only_all_node_cell_and_recreated_range_inputs_reject_retained_native_handlers(
    cx: &mut TestAppContext,
) {
    let (document, blocks, cells) = table();
    let calls = Rc::new(Calls::default());
    let editor = instance(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        true,
        calls.clone(),
    );
    let session = editor.session().clone();
    let handle = open(&editor, cx);
    let mut retained = Vec::new();
    for selection in [
        DocumentSelection::all(&document),
        DocumentSelection::node(&document, blocks[0]).unwrap(),
        DocumentSelection::cell_range(
            cells[0],
            cells[1],
            InlinePoint::at_start_of(blocks[0]).into(),
        ),
        DocumentSelection::node(&document, blocks[1]).unwrap(),
    ] {
        session
            .borrow_mut()
            .set_document_selection(selection)
            .unwrap();
        handle
            .update(cx, |view, window, cx| {
                view.focus_selection(window, cx);
                let input = view.range_input.as_ref().unwrap().1.clone();
                retained.push(input.clone());
                let before = Snapshot::capture(&session, &calls);
                exercise_native(&input, window, cx);
                before.assert_unchanged(&session, &calls);
            })
            .unwrap();
        cx.simulate_input(handle.into(), "range typing");
    }
    session
        .borrow_mut()
        .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
            blocks[1],
        )))
        .unwrap();
    handle
        .update(cx, |view, window, cx| {
            view.focus_selection(window, cx);
            let before = Snapshot::capture(&session, &calls);
            for input in &retained {
                input.update(cx, |input, cx| {
                    input.replace_and_mark_text_in_range(None, "late preedit", None, window, cx);
                    input.replace_text_in_range(None, "late commit", window, cx);
                    input.unmark_text(window, cx);
                    assert!(!input.is_composing());
                });
            }
            exercise_native(&view.children[1].1.clone(), window, cx);
            before.assert_unchanged(&session, &calls);
        })
        .unwrap();
    let before = Snapshot::capture(&session, &calls);
    // Last-cell Tab would normally append a row. Read-only must not do that.
    cx.simulate_keystrokes(handle.into(), "tab");
    before.assert_unchanged(&session, &calls);
    assert_eq!(calls.router.get(), 0);
}

#[gpui::test]
fn read_only_host_updates_remain_available_and_every_new_child_inherits_mode(
    cx: &mut TestAppContext,
) {
    let (original, blocks) = document(2);
    let calls = Rc::new(Calls::default());
    let editor = instance(
        original,
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        true,
        calls.clone(),
    );
    let session = editor.session().clone();
    let handle = open(&editor, cx);
    let (replacement, _) = document(70);
    let transaction =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::ReplaceDocument {
            template: DocumentTemplate::capture(&replacement).unwrap(),
        });
    let plan = EditPlan::new(transaction, SelectionUpdate::CaretAtDocumentEnd, None);
    let retired = handle
        .update(cx, |view, window, cx| {
            let retired = view.children[0].1.clone();
            assert_eq!(
                view.apply_passive_edit_plan(&plan, window, cx).unwrap(),
                Some(SessionOutcome::DocumentChanged)
            );
            assert_eq!(view.children.len(), 70);
            retired
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, window, cx| {
            for (node, child) in view.children.clone() {
                session
                    .borrow_mut()
                    .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
                        node,
                    )))
                    .unwrap();
                let before = Snapshot::capture(&session, &calls);
                exercise_native(&child, window, cx);
                before.assert_unchanged(&session, &calls);
            }
            retired.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "retired", None, window, cx);
                input.replace_text_in_range(None, "retired", window, cx);
                assert!(!input.is_composing());
            });
            let node = view.children[0].0;
            let update = Transaction::new(TransactionOrigin::System).with_step(
                TransactionStep::SetNodeAttrs {
                    node,
                    attrs: NodeAttrs::new([("host".into(), AttrValue::Bool(true))].into()).unwrap(),
                },
            );
            assert_eq!(
                view.apply_edit_transaction(&update, window, cx).unwrap(),
                Some(SessionOutcome::DocumentChanged)
            );
            assert_eq!(
                session
                    .borrow()
                    .document()
                    .node(node)
                    .unwrap()
                    .attrs()
                    .get("host"),
                Some(&AttrValue::Bool(true))
            );
        })
        .unwrap();
    assert_eq!(session.borrow().history_depths(), (2, 0));
    let before = Snapshot::capture(&session, &calls);
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    before.assert_unchanged(&session, &calls);
    // Building another view later inherits the same immutable instance mode.
    assert!(editor.build_view().is_read_only_input());
}

#[gpui::test]
fn read_only_checkbox_and_column_resize_do_not_commit_or_call_host_guards(cx: &mut TestAppContext) {
    let (document, items, blocks) = crate::document_view::task_checkbox_tests::fixture();
    let calls = Rc::new(Calls::default());
    let editor = instance(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[1])),
        true,
        calls.clone(),
    );
    let session = editor.session().clone();
    let handle = open(&editor, cx);
    let before = Snapshot::capture(&session, &calls);
    for item in items {
        let selector = Box::leak(format!("task-checkbox-{item:?}").into_boxed_str());
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        let p = visual.debug_bounds(selector).unwrap().center();
        visual.simulate_mouse_down(p, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(p, MouseButton::Left, Default::default());
        before.assert_unchanged(&session, &calls);
    }

    let (document, blocks, _) = table();
    let editor = instance(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        true,
        calls.clone(),
    );
    let session = editor.session().clone();
    let guards = Rc::new(Cell::new(0));
    let commits = Rc::new(Cell::new(0));
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| {
                let mut view = editor.build_view();
                view.set_measured_table_layout(true);
                let guard_calls = guards.clone();
                let commit_calls = commits.clone();
                view.set_table_column_resize(Some(TableColumnResize::new(
                    TableColumnResizeConfig {
                        handle_width: 5.0,
                        min_column_width: 25,
                        last_column_resizable: true,
                    },
                    move |_, _| {
                        guard_calls.set(guard_calls.get() + 1);
                        true
                    },
                    move |_, _, _, _| {
                        commit_calls.set(commit_calls.get() + 1);
                    },
                )));
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
    let edge = handle
        .update(cx, |view, _, _| {
            let cell = view.cell_registry.borrow()[0].1;
            point(cell.right(), cell.top() + px(20.0))
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.simulate_mouse_down(edge, MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        edge + point(px(40.0), px(0.0)),
        Some(MouseButton::Left),
        Default::default(),
    );
    visual.simulate_mouse_up(
        edge + point(px(40.0), px(0.0)),
        MouseButton::Left,
        Default::default(),
    );
    cx.background_executor.run_until_parked();
    assert_eq!(guards.get(), 0);
    assert_eq!(commits.get(), 0);
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
    handle
        .update(cx, |view, _, _| {
            assert!(!view.has_pending_table_column_resize())
        })
        .unwrap();
}

#[gpui::test]
fn read_only_selection_copy_and_reading_highlights_reveal_still_work(cx: &mut TestAppContext) {
    let (document, blocks) = document(70);
    let calls = Rc::new(Calls::default());
    let editor = instance(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        true,
        calls.clone(),
    );
    let session = editor.session().clone();
    let handle = open(&editor, cx);
    cx.simulate_keystrokes(handle.into(), "shift-right");
    assert!(!session.borrow().selection().is_collapsed());
    cx.simulate_keystrokes(handle.into(), "ctrl-c");
    cx.update(|cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("h")
        )
    });
    cx.simulate_keystrokes(handle.into(), "ctrl-a");
    cx.simulate_keystrokes(handle.into(), "ctrl-c");
    cx.update(|cx| {
        assert_eq!(
            cx.read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .matches("hello")
                .count(),
            70
        )
    });
    cx.simulate_keystrokes(handle.into(), "home");
    assert!(session.borrow().selection().is_collapsed());
    let before = Snapshot::capture(&session, &calls);
    let range = ReadingRange::new(
        InlinePoint::at_start_of(blocks[60]),
        InlinePoint::new(
            blocks[60],
            session
                .borrow()
                .document()
                .node(blocks[60])
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .offset_at(5)
                .unwrap(),
            0,
            CursorAffinity::After,
        ),
    );
    let external = cx.update(|cx| cx.focus_handle());
    handle
        .update(cx, |view, window, cx| {
            window.focus(&external);
            let snapshot = view.reading_snapshot();
            view.set_reading_highlights(&snapshot, &[range], Some(0), cx)
                .unwrap();
            view.reveal_range(&snapshot, range, window, cx).unwrap();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, window, cx| {
            assert!(external.is_focused(window));
            assert_eq!(
                view.reading_reveal_status(),
                Some(ReadingRevealStatus::Revealed)
            );
            assert!(view.scroll_handle.offset().y < px(0.0));
            assert_eq!(view.reading.borrow().highlights.len(), 1);
            view.clear_reading_highlights(cx);
            assert!(view.reading.borrow().highlights.is_empty());
        })
        .unwrap();
    before.assert_unchanged(&session, &calls);
    assert_eq!(session.borrow().document().store(), document.store());
}

struct KeepDocument(XiaomuDocument);
impl SessionPolicy for KeepDocument {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.store() == self.0.store() {
            Ok(())
        } else {
            Err(PolicyError::new("host update denied"))
        }
    }
}
#[gpui::test]
fn read_only_input_does_not_replace_session_policy_for_host_transactions(cx: &mut TestAppContext) {
    let (document, blocks) = document(1);
    let editor = EditorInstance::new_with_policy(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        EditorHooks::default(),
        Box::new(KeepDocument(document.clone())),
    )
    .unwrap()
    .with_read_only_input();
    let handle = open(&editor, cx);
    let update =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::SetNodeAttrs {
            node: blocks[0],
            attrs: NodeAttrs::new([("host".into(), AttrValue::Bool(true))].into()).unwrap(),
        });
    handle
        .update(cx, |view, window, cx| {
            assert!(view.apply_edit_transaction(&update, window, cx).is_err())
        })
        .unwrap();
    assert_eq!(
        editor.session().borrow().document().store(),
        document.store()
    );
    assert_eq!(editor.session().borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn read_only_idle_measurement_does_not_invalidate_view(cx: &mut TestAppContext) {
    let (document, blocks, _) = table();
    let editor = instance(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        true,
        Rc::new(Calls::default()),
    );
    let handle = open(&editor, cx);
    handle
        .update(cx, |view, _, cx| {
            view.set_measured_table_layout(true);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    let _subscription = handle
        .update(cx, |_, _, cx| {
            let entity = cx.entity();
            cx.observe(&entity, move |_, _, _| observed.set(observed.get() + 1))
        })
        .unwrap();
    // Every measured prepaint reaches this callback, even without a resize
    // capability. An idle read-only view must count as valid, not cancel and
    // notify itself again on every frame.
    for _ in 0..3 {
        handle
            .update(cx, |view, window, cx| {
                assert!(view.validate_column_resize(cx));
                view.finish_column_resize_measurement(window, cx);
            })
            .unwrap();
        cx.background_executor.run_until_parked();
    }
    assert_eq!(notifications.get(), 0);
}
