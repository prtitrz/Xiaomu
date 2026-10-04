//! Host-facing presentation admission follows each instance's real measurement.

use super::*;

#[test]
fn presentation_guard_requires_opt_in_and_measurement_only_when_tables_exist() {
    let fixture = fixture(Some("rowspan"));
    let mut view = DocumentView::new(session(&fixture, fixture.before));
    let guard = view.measured_table_presentation_guard();
    assert!(!guard(&fixture.document));
    let without_table = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode {
            node: fixture.table,
        })
        .apply(&fixture.document)
        .unwrap();
    assert!(guard(&without_table));
    view.set_measured_table_layout(true);
    assert!(!guard(&fixture.document), "opt-in alone is not measurement");
    assert!(guard(&without_table));
}

#[gpui::test]
fn presentation_guard_allows_unpainted_typing_and_rejects_invalid_width_until_undo(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(Some("rowspan"));
    let session = session(&fixture, fixture.blocks[0]);
    let handle = open(session.clone(), cx);
    let guard = handle
        .update(cx, |view, _, _| view.measured_table_presentation_guard())
        .unwrap();
    assert!(guard(session.borrow().document()));
    handle
        .update(cx, |view, window, cx| {
            let input = view.focused_child(window, cx).unwrap();
            input.update(cx, |input, cx| {
                for value in ["a", "b"] {
                    input.replace_text_in_range(None, value, window, cx);
                    // The host can immediately prepare another normal edit
                    // or save the current snapshot before this update ends.
                    assert!(guard(session.borrow().document()));
                }
            });
        })
        .unwrap();
    assert_eq!(text(&session, fixture.blocks[0]), "abhead");
    let typed = session.borrow().document().clone();
    session
        .borrow_mut()
        .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
            fixture.before,
        )))
        .unwrap();
    session
        .borrow_mut()
        .apply(&Transaction::new(TransactionOrigin::System).with_step(
            TransactionStep::RemoveNode {
                node: fixture.table,
            },
        ))
        .unwrap();
    assert!(guard(session.borrow().document()), "last table was removed");
    // History recovery must bypass a host's normal-edit gate. No draw occurs
    // between deletion, this guard call and restoration of the original IDs.
    session.borrow_mut().undo().unwrap();
    assert_eq!(session.borrow().document().store(), typed.store());
    assert!(
        !guard(session.borrow().document()),
        "deleted success was pruned"
    );
    repaint(handle, cx);
    assert!(guard(session.borrow().document()));
    let attrs = NodeAttrs::new(
        [(
            "colwidth".to_owned(),
            AttrValue::List(vec![AttrValue::Integer(137)]),
        )]
        .into(),
    )
    .unwrap();
    session
        .borrow_mut()
        .apply(&Transaction::new(TransactionOrigin::System).with_step(
            TransactionStep::SetNodeAttrs {
                node: fixture.cells[2],
                attrs,
            },
        ))
        .unwrap();
    assert!(
        session
            .borrow()
            .document()
            .table_grid(fixture.table)
            .is_ok()
    );
    assert!(session.borrow().document().validate().is_ok());
    assert!(!guard(session.borrow().document()), "no intervening draw");
    repaint(handle, cx);
    assert!(!guard(session.borrow().document()));
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    cx.background_executor.run_until_parked();
    assert_eq!(session.borrow().document().store(), typed.store());
    assert!(guard(session.borrow().document()));
    handle
        .update(cx, |view, _, _| view.set_measured_table_layout(false))
        .unwrap();
    assert!(
        !guard(session.borrow().document()),
        "captured guard sees revocation"
    );
}

fn open_sized(session: SharedSession, cx: &mut TestAppContext) -> WindowHandle<SizedHost> {
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let editor = cx.new(|_| {
                let mut view = DocumentView::new(session);
                view.set_measured_table_layout(true);
                view
            });
            cx.new(|_| SizedHost {
                editor,
                width: 400.0,
            })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    handle
}

#[gpui::test]
fn presentation_guard_tracks_real_height_and_width_failures_per_pane_and_recovers(
    cx: &mut TestAppContext,
) {
    let mut fixture = fixture(Some("rowspan"));
    // A rule's real .my_3() margins depend on the window rem size. Inflating
    // that size below exceeds the table's measured-height limit without
    // changing the canonical document or the checked width/placement plan.
    fixture.document = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::InsertNode {
            parent: fixture.cells[0],
            index: 1,
            kind: NodeKind::HorizontalRule,
            attrs: NodeAttrs::empty(),
            content: NodeContent::Atomic,
        })
        .apply(&fixture.document)
        .unwrap();
    let session = session(&fixture, fixture.blocks[0]);
    let first = open_sized(session.clone(), cx);
    let second = open_sized(session.clone(), cx);
    let first_guard = first
        .update(cx, |host, _, cx| {
            host.editor.read(cx).measured_table_presentation_guard()
        })
        .unwrap();
    let second_guard = second
        .update(cx, |host, _, cx| {
            host.editor.read(cx).measured_table_presentation_guard()
        })
        .unwrap();
    let document = session.borrow().document().clone();
    assert!(first_guard(&document));
    assert!(second_guard(&document));
    let plan = crate::table_layout::TableLayoutPlan::from_document(
        &document,
        fixture.table,
        Default::default(),
    )
    .unwrap();
    assert!(plan.layout(0.0, &vec![0.0; plan.cells().len()]).is_ok());
    let original_rem = first
        .update(cx, |_, window, cx| {
            let original = window.rem_size();
            window.set_rem_size(px(800_000.0));
            cx.notify();
            original
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    first
        .update(cx, |host, _, cx| {
            let view = host.editor.read(cx);
            assert!(
                view.table_capability
                    .borrow()
                    .can_build(&document, fixture.table)
            );
            assert!(view.cell_registry.borrow().is_empty());
        })
        .unwrap();
    assert!(
        !first_guard(&document),
        "real child height revoked this pane"
    );
    assert!(
        second_guard(&document),
        "another pane measured successfully"
    );
    assert_eq!(session.borrow().document().store(), document.store());
    first
        .update(cx, |_, window, cx| {
            window.set_rem_size(original_rem);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(first_guard(&document));
    for (width, expected) in [(1_000_100.0, false), (400.0, true)] {
        first
            .update(cx, |host, _, cx| {
                host.width = width;
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        assert_eq!(first_guard(&document), expected);
        assert!(second_guard(&document));
    }
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
