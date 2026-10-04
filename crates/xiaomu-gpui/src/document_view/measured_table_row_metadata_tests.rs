//! Exact host metadata remains inert while measured admission stays per view.

use super::*;

const MARKER: &str = "example:empty-content";

fn metadata() -> NodeAttrs {
    NodeAttrs::new([(MARKER.to_owned(), AttrValue::Bool(true))].into()).unwrap()
}

fn covered_empty_row_fixture() -> (Fixture, NodeId) {
    let mut fixture = fixture(Some("rowspan"));
    let second_row = fixture
        .document
        .node(fixture.table)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[1];
    let attrs = NodeAttrs::new(
        fixture
            .document
            .node(fixture.cells[1])
            .unwrap()
            .attrs()
            .iter()
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .chain([("rowspan".to_owned(), AttrValue::Integer(2))])
            .collect(),
    )
    .unwrap();
    let removed = fixture.cells.pop().unwrap();
    fixture.blocks.pop();
    fixture.document = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::SetNodeAttrs {
            node: fixture.cells[1],
            attrs,
        })
        .with_step(TransactionStep::RemoveNode { node: removed })
        .with_step(TransactionStep::SetNodeAttrs {
            node: second_row,
            attrs: metadata(),
        })
        .apply(&fixture.document)
        .unwrap();
    assert!(
        fixture
            .document
            .node(second_row)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .is_empty()
    );
    (fixture, second_row)
}

#[gpui::test]
fn measured_row_metadata_requires_exact_configuration_and_fresh_layout_per_view(
    cx: &mut TestAppContext,
) {
    let (fixture, empty_row) = covered_empty_row_fixture();
    let session = session(&fixture, fixture.blocks[0]);
    let first = open(session.clone(), cx);
    let second = open(session.clone(), cx);
    let first_guard = first
        .update(cx, |view, _, _| view.measured_table_presentation_guard())
        .unwrap();
    let second_guard = second
        .update(cx, |view, _, _| view.measured_table_presentation_guard())
        .unwrap();
    assert!(!first_guard(session.borrow().document()));
    assert!(!second_guard(session.borrow().document()));
    let selector =
        Box::leak(format!("unsupported-measured-table-{:?}", fixture.table).into_boxed_str());
    assert!(
        VisualTestContext::from_window(first.into(), cx)
            .debug_bounds(selector)
            .is_some()
    );
    first
        .update(cx, |view, _, cx| {
            view.set_measured_table_row_metadata(metadata());
            assert!(!first_guard(session.borrow().document()));
            assert!(
                view.table_capability
                    .borrow()
                    .can_build(session.borrow().document(), fixture.table)
            );
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(first_guard(session.borrow().document()));
    assert!(!second_guard(session.borrow().document()));
    let (paragraph, proxy) = first
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
            let paragraph = view.focused_child(window, cx).unwrap();
            view.install_cell_range(fixture.cells[0], fixture.cells[1], window, cx);
            let proxy = view.focused_child(window, cx).unwrap();
            view.place(InlinePoint::at_start_of(fixture.blocks[0]), window, cx);
            (paragraph, proxy)
        })
        .unwrap();
    let before = session.borrow().document().clone();
    first
        .update(cx, |view, window, cx| {
            for allowed in [NodeAttrs::empty(), metadata()] {
                view.set_measured_table_row_metadata(allowed);
                // Changing back to the same whitelist still needs a new
                // measurement; neither retained callback may reuse success.
                assert!(!first_guard(session.borrow().document()));
                for input in [&paragraph, &proxy] {
                    input.update(cx, |input, cx| {
                        input.replace_text_in_range(None, "late", window, cx);
                        input.replace_and_mark_text_in_range(
                            None,
                            "late preedit",
                            None,
                            window,
                            cx,
                        );
                        assert!(input.selected_text_range(false, window, cx).is_none());
                        assert!(!input.is_composing());
                    });
                }
            }
            cx.notify();
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
    cx.background_executor.run_until_parked();
    assert!(first_guard(session.borrow().document()));
    assert!(!second_guard(session.borrow().document()));
    cx.simulate_input(first.into(), "ok");
    assert_eq!(text(&session, fixture.blocks[0]), "okhead");
    second
        .update(cx, |view, _, cx| {
            view.set_measured_table_row_metadata(metadata());
            assert!(!second_guard(session.borrow().document()));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(second_guard(session.borrow().document()));
    first
        .update(cx, |view, _, _| {
            view.set_measured_table_row_metadata(NodeAttrs::empty())
        })
        .unwrap();
    assert!(!first_guard(session.borrow().document()));
    assert!(second_guard(session.borrow().document()));
    let state = session.borrow();
    let row = state.document().node(empty_row).unwrap();
    assert_eq!(row.attrs(), &metadata());
    assert!(row.content().as_children().unwrap().is_empty());
}
