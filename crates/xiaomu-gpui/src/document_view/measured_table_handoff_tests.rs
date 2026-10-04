//! Native range-to-inline delivery before redraw, then real input ownership.

use super::*;

#[gpui::test]
fn measured_cell_range_batch_handoff_repaints_real_queries_and_composition(
    cx: &mut TestAppContext,
) {
    for span in ["rowspan", "colspan"] {
        let fixture = fixture(Some(span));
        let session = session(&fixture, fixture.blocks[0]);
        let handle = open(session.clone(), cx);
        handle
            .update(cx, |view, window, cx| {
                view.install_cell_range(fixture.cells[0], fixture.cells[2], window, cx);
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        let range_selection = session.borrow().selection();
        assert!(range_selection.active_cell_range().is_some());

        let (proxy, target) = handle
            .update(cx, |view, window, cx| {
                assert!(view.range_input_is_focused(window, cx));
                let proxy = view.focused_child(window, cx).unwrap();
                let target = proxy.update(cx, |input, cx| {
                    assert_eq!(
                        input.selected_text_range(false, window, cx).unwrap().range,
                        0..0
                    );
                    input.replace_text_in_range(None, "a", window, cx);
                    let target = {
                        let state = session.borrow();
                        assert!(state.selection().active_cell_range().is_none());
                        let (anchor, focus) = state.selection().as_same_node_inline().unwrap();
                        assert_eq!(anchor, focus);
                        assert_eq!(focus.text_offset().as_usize(), 1);
                        focus.node_id()
                    };
                    assert_eq!(text(&session, target), "a");
                    // Both native callbacks use this same mounted proxy in
                    // one entity update, without a draw or session mutation
                    // in between. The newly allocated paragraph is not yet
                    // materialized, so revision-only admission loses this b.
                    input.replace_text_in_range(None, "b", window, cx);
                    assert_eq!(text(&session, target), "ab");
                    target
                });
                assert!(view.children.iter().all(|(node, _)| *node != target));
                assert_eq!(
                    view.focused_child(window, cx).unwrap().entity_id(),
                    proxy.entity_id()
                );
                (proxy, target)
            })
            .unwrap();
        // Cell replacement is isolated; the next ordinary character starts
        // a separate typing unit. Neither callback is silently discarded.
        assert_eq!(session.borrow().history_depths(), (2, 0));
        let after_batch = session.borrow().document().clone();
        let batch_selection = session.borrow().selection();
        for cell in &fixture.cells {
            assert_eq!(
                after_batch.node(*cell).unwrap().attrs(),
                fixture.document.node(*cell).unwrap().attrs()
            );
        }
        cx.background_executor.run_until_parked();

        let input = handle
            .update(cx, |view, window, cx| {
                assert!(!view.uses_range_input());
                assert!(view.range_input.is_none());
                assert!(!proxy.read(cx).focus_handle(cx).is_focused(window));
                let input = view.focused_child(window, cx).unwrap();
                assert_ne!(input.entity_id(), proxy.entity_id());
                assert_eq!(input.read(cx).node(), target);
                let block = view.block_bounds(target).unwrap();
                let cell = view
                    .cell_registry
                    .borrow()
                    .iter()
                    .find(|(cell, _)| *cell == fixture.cells[0])
                    .unwrap()
                    .1;
                assert_eq!(block.left(), cell.left() + px(12.0));
                assert_eq!(block.top(), cell.top() + px(9.0));
                input.update(cx, |input, cx| {
                    // Only the newly painted inline handler answers these
                    // queries. The retired proxy's 0..0 is not caret proof.
                    assert_eq!(
                        input.selected_text_range(false, window, cx).unwrap().range,
                        2..2
                    );
                    let mut adjusted = None;
                    assert_eq!(
                        input.text_for_range(0..2, &mut adjusted, window, cx),
                        Some("ab".to_owned())
                    );
                    assert_eq!(adjusted, Some(0..2));
                    let caret = input.bounds_for_range(2..2, block, window, cx).unwrap();
                    assert_eq!(caret.top(), block.top());
                    assert!(caret.left() > block.left());
                    assert!(caret.right() <= block.right());
                    assert_eq!(
                        input.character_index_for_point(
                            caret.origin + point(px(0.1), px(1.0)),
                            window,
                            cx
                        ),
                        Some(2)
                    );
                    input.replace_and_mark_text_in_range(None, "中🙂", Some(3..3), window, cx);
                    assert!(input.is_composing());
                    assert_eq!(input.marked_text_range(window, cx), Some(2..5));
                });
                input
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        assert_eq!(session.borrow().document().store(), after_batch.store());
        assert_eq!(session.borrow().selection(), batch_selection);
        assert_eq!(session.borrow().history_depths(), (2, 0));
        handle
            .update(cx, |view, window, cx| {
                assert_eq!(
                    view.focused_child(window, cx).unwrap().entity_id(),
                    input.entity_id()
                );
                let block = view.block_bounds(target).unwrap();
                input.update(cx, |input, cx| {
                    assert_eq!(input.marked_text_range(window, cx), Some(2..5));
                    assert_eq!(
                        input.selected_text_range(false, window, cx).unwrap().range,
                        5..5
                    );
                    let candidate = input.bounds_for_range(2..5, block, window, cx).unwrap();
                    assert!(candidate.left() > block.left());
                    assert_eq!(candidate.top(), block.top());
                    assert!(candidate.right() <= block.right());
                    assert!(candidate.bottom() <= block.bottom());
                    assert!(candidate.size.width > px(1.0));
                });
            })
            .unwrap();
        cx.dispatch_keystroke(handle.into(), gpui::Keystroke::parse("x->中🙂").unwrap());
        cx.background_executor.run_until_parked();
        assert_eq!(text(&session, target), "ab中🙂");
        assert_eq!(session.borrow().history_depths(), (3, 0));
        handle
            .update(cx, |view, window, cx| {
                assert!(!view.has_active_composition(cx));
                assert_eq!(
                    view.focused_child(window, cx).unwrap().entity_id(),
                    input.entity_id()
                );
            })
            .unwrap();
        cx.simulate_keystrokes(handle.into(), "ctrl-z");
        assert_eq!(session.borrow().document().store(), after_batch.store());
        assert_eq!(session.borrow().selection(), batch_selection);
        cx.simulate_keystrokes(handle.into(), "ctrl-z");
        assert_eq!(text(&session, target), "a");
        cx.simulate_keystrokes(handle.into(), "ctrl-z");
        assert_eq!(
            session.borrow().document().store(),
            fixture.document.store()
        );
        assert_eq!(session.borrow().selection(), range_selection);
    }
}
