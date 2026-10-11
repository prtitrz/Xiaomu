//! Additional mounted first-line inset regression contracts.
use super::*;

#[gpui::test]
fn accepted_inset_preedit_commit_and_history_preserve_canonical_unicode(cx: &mut TestAppContext) {
    let (document, node) = document(&[("base", None)], None);
    let before = document.clone();
    let (handle, session) = mount(cx, document, node, 95., Some(36.), BlockAlignment::Left);
    let selection = session.borrow().selection();
    handle
        .update(cx, |host, window, cx| {
            host.input.update(cx, |view, cx| {
                view.replace_and_mark_text_in_range(None, "中🙂e\u{301}", Some(5..5), window, cx);
                assert!(view.is_composing());
                assert_eq!(view.canonical_text(), "base");
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            host.input.update(cx, |view, cx| {
                near(
                    view.last_layout
                        .as_ref()
                        .unwrap()
                        .position_for_caret(0, CursorAffinity::Before)
                        .unwrap()
                        .x,
                    36.,
                );
                view.replace_text_in_range(None, "中🙂e\u{301}", window, cx);
                assert!(!view.is_composing());
                assert_eq!(view.canonical_text(), "中🙂e\u{301}base");
            })
        })
        .unwrap();
    assert_eq!(session.borrow().history_depths(), (1, 0));
    let committed = session.borrow().document().clone();
    session.borrow_mut().undo().unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().selection(), selection);
    session.borrow_mut().redo().unwrap();
    assert_eq!(session.borrow().document().store(), committed.store());
}

#[gpui::test]
fn empty_inset_uses_existing_engine_row_contract_and_no_selection_ink(cx: &mut TestAppContext) {
    // Original browser evidence measured only trailing BR, never its caret.
    // This asserts the native engine's one empty row contract, not browser parity.
    let (document, node) = document(&[], None);
    let (handle, session) = mount(
        cx,
        document.clone(),
        node,
        20.,
        Some(36.),
        BlockAlignment::Left,
    );
    let selection = session.borrow().selection();
    handle
        .update(cx, |host, window, cx| {
            host.input.update(cx, |view, cx| {
                let layout = view.last_layout.as_ref().unwrap();
                let start = layout
                    .position_for_caret(0, CursorAffinity::Before)
                    .unwrap();
                near(start.x, 36.);
                near(start.y, 0.);
                assert!(layout.selection_rects(0..0).is_empty());
                assert!(layout.size().height > px(0.));
                assert_eq!(
                    layout.caret_for_position(start + point(px(0.), px(1.))),
                    (0, CursorAffinity::After)
                );
                assert_eq!(
                    view.selected_text_range(true, window, cx).unwrap().range,
                    0..0
                );
            })
        })
        .unwrap();
    unchanged(&session, &document, selection);
}

#[gpui::test]
fn visual_home_end_and_desired_x_navigation_follow_first_and_continuation_rows(
    cx: &mut TestAppContext,
) {
    let (document, node) = document(&[("ABCDEFGHIJKLMN OPQRSTUVWXYZ more rows", None)], None);
    let before = document.clone();
    let (handle, session) = mount(cx, document, node, 95., Some(36.), BlockAlignment::Left);
    let selection = session.borrow().selection();
    handle
        .update(cx, |host, _, cx| {
            let view = host.input.read(cx);
            let layout = view.last_layout.as_ref().unwrap();
            let first_end = view
                .visual_line_edge_target(0, CursorAffinity::Before, true)
                .unwrap();
            assert!(first_end.0 > 0);
            assert_eq!(first_end.1, CursorAffinity::Before);
            assert!(view.visual_is_soft_wrap_boundary(first_end.0));
            assert_eq!(
                view.visual_line_edge_target(first_end.0, CursorAffinity::Before, false),
                Some((0, CursorAffinity::Before))
            );
            let next_home = view
                .visual_line_edge_target(first_end.0, CursorAffinity::After, false)
                .unwrap();
            assert_eq!(next_home, (first_end.0, CursorAffinity::After));
            near(view.visual_caret_x(next_home.0, next_home.1).unwrap(), 0.);
            let desired_x = view.visual_caret_x(0, CursorAffinity::Before).unwrap();
            near(desired_x, 36.);
            let down = view
                .visual_vertical_target(0, CursorAffinity::Before, desired_x, true)
                .unwrap();
            assert_ne!(
                down, next_home,
                "vertical motion preserves x, not row-start byte"
            );
            let next_y = layout
                .position_for_caret(next_home.0, next_home.1)
                .unwrap()
                .y;
            assert_eq!(
                down,
                layout.caret_for_position(point(desired_x, next_y + px(1.)))
            );
            assert_eq!(
                view.visual_vertical_target(down.0, down.1, desired_x, false),
                Some((0, CursorAffinity::Before))
            );
            let next_end = view.visual_line_edge_target(down.0, down.1, true).unwrap();
            assert!(next_end.0 > next_home.0);
            assert_eq!(
                view.visual_line_edge_target(next_end.0, CursorAffinity::Before, false),
                Some(next_home)
            );
        })
        .unwrap();
    unchanged(&session, &before, selection);
}
