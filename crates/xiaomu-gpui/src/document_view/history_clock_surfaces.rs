//! Real virtual-platform routing over nested/table/range children; no OS GUI.
use super::*;

#[gpui::test]
fn nested_and_measured_table_children_share_the_editor_clock(cx: &mut TestAppContext) {
    let opened = open(cx);
    assert_eq!(
        opened.clock.samples.get(),
        0,
        "materializing or painting never samples"
    );
    let nodes = [
        opened.fixture.before,
        opened.fixture.nested,
        opened.fixture.cell_texts[0],
        opened.fixture.cell_texts[1],
        opened.fixture.cell_texts[2],
    ];
    for (index, node) in nodes.into_iter().enumerate() {
        select(&opened, node, 0, cx);
        let time = 100_000 + 10_000 * index as u64;
        opened.clock.milliseconds.set(time);
        cx.simulate_input(opened.handle.into(), "中");
        opened.clock.milliseconds.set(time + 500);
        cx.simulate_input(opened.handle.into(), "🙂");
        assert_eq!(text(&opened.session, node), "中🙂");
        assert_eq!(opened.clock.samples.get(), 2 * (index + 1));
        assert_eq!(
            opened.session.borrow().history_depths(),
            (index + 1, 0),
            "each child supplies adjacent stamps from the one injected clock"
        );
    }
    for node in nodes.into_iter().rev() {
        opened.session.borrow_mut().undo().unwrap();
        assert_eq!(text(&opened.session, node), "");
    }
    for node in nodes {
        opened.session.borrow_mut().redo().unwrap();
        assert_eq!(text(&opened.session, node), "中🙂");
    }
    assert_eq!(
        opened.clock.samples.get(),
        10,
        "Runtime traversal never consults the frontend clock"
    );
}

#[gpui::test]
fn all_whole_node_and_cell_range_proxies_inherit_without_refreshing_edit_time(
    cx: &mut TestAppContext,
) {
    let opened = open(cx);
    opened.clock.milliseconds.set(1000);
    cx.simulate_input(opened.handle.into(), "a");
    let caret = opened.session.borrow().selection();
    opened.clock.milliseconds.set(5000);
    // These policy-NoChange inputs intentionally test clock propagation, not
    // unsupported default All/node/span editing semantics. They traverse the
    // platform's installed handler for each newly materialized range proxy.
    for kind in 0..3 {
        {
            let mut session = opened.session.borrow_mut();
            match kind {
                0 => {
                    let all = DocumentSelection::all(session.document());
                    session.set_document_selection(all).unwrap();
                }
                1 => {
                    session.set_node_selection(opened.fixture.atomic).unwrap();
                }
                _ => {
                    session
                        .set_cell_range_selection(opened.fixture.cells[0], opened.fixture.cells[0])
                        .unwrap();
                }
            }
        }
        opened
            .handle
            .update(cx, |view, window, cx| {
                view.focus_selection(window, cx);
                assert!(view.range_input_is_focused(window, cx));
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        let snapshot = Snapshot::capture(&opened);
        cx.simulate_input(opened.handle.into(), "?");
        assert_eq!(opened.clock.samples.get(), kind + 2);
        snapshot.assert_unchanged(&opened);
    }
    opened
        .session
        .borrow_mut()
        .set_document_selection(caret)
        .unwrap();
    opened
        .handle
        .update(cx, |view, window, cx| view.focus_selection(window, cx))
        .unwrap();
    cx.background_executor.run_until_parked();
    opened.clock.milliseconds.set(1500);
    cx.simulate_input(opened.handle.into(), "b");
    assert_eq!(opened.clock.samples.get(), 5);
    assert_eq!(text(&opened.session, opened.fixture.before), "ab");
    assert_eq!(opened.session.borrow().history_depths(), (1, 0));
}
