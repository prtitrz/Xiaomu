//! Fresh tables must earn measured admission before their input can own focus.

use super::*;
use xiaomu_core::document::AttrValue;

#[gpui::test]
fn passive_fresh_table_earns_measurement_and_unsupported_presentation_stays_protected(
    cx: &mut TestAppContext,
) {
    for supported in [false, true] {
        let original = document(2);
        let a = editor(original.clone(), Default::default());
        let b = editor(original.clone(), Default::default());
        let session = a.session().clone();
        let handle = open_panes(cx, &a, &b);
        handle
            .update(cx, |panes, window, cx| {
                panes.a.update(cx, |view, cx| {
                    view.set_measured_table_layout(true);
                    view.focus_selection(window, cx);
                });
            })
            .unwrap();
        cx.background_executor.run_until_parked();

        let mut transaction = Transaction::new(TransactionOrigin::System);
        for &node in children(&original) {
            transaction = transaction.with_step(TransactionStep::RemoveNode { node });
        }
        transaction = transaction.with_step(TransactionStep::InsertTable {
            parent: original.root(),
            index: 0,
            rows: 1,
            columns: 1,
        });
        let candidate = transaction.apply(&original).unwrap();
        let table = children(&candidate)[0];
        if !supported {
            transaction = transaction.with_step(TransactionStep::SetNodeAttrs {
                node: table,
                attrs: NodeAttrs::new([("unsupported".into(), AttrValue::Bool(true))].into())
                    .unwrap(),
            });
        }
        let plan = EditPlan::new(transaction, SelectionUpdate::CaretAtDocumentEnd, None);
        handle
            .update(cx, |panes, window, cx| {
                panes.a.update(cx, |view, cx| {
                    assert_eq!(
                        view.apply_passive_edit_plan(&plan, window, cx).unwrap(),
                        Some(SessionOutcome::DocumentChanged)
                    );
                    assert!(!(view.measured_table_presentation_guard())(
                        session.borrow().document()
                    ));
                    assert!(view.focus_handle.as_ref().unwrap().is_focused(window));
                    assert!(view.focused_child(window, cx).is_none());
                });
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |panes, window, cx| {
                let view = panes.a.read(cx);
                assert_eq!(
                    (view.measured_table_presentation_guard())(session.borrow().document()),
                    supported
                );
                assert_eq!(view.focused_child(window, cx).is_some(), supported);
                assert_eq!(
                    view.focus_handle.as_ref().unwrap().is_focused(window),
                    !supported
                );
            })
            .unwrap();
        cx.simulate_input(handle.into(), "T");
        cx.background_executor.run_until_parked();
        assert_eq!(focus_text(&session), if supported { "T" } else { "" });
        assert_eq!(
            session.borrow().history_depths(),
            if supported { (2, 0) } else { (1, 0) }
        );
        assert!(session.borrow().document().node(table).is_some());
    }
}
