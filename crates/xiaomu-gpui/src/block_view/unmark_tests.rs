//! Unmark is distinct from explicit empty cancellation and focus loss.
use super::ime_atom_tests::open;
use super::*;
use gpui::{EntityInputHandler, TestAppContext};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};

fn text(session: &SharedSession, node: NodeId) -> String {
    session
        .borrow()
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

#[cfg(target_os = "linux")]
#[gpui::test]
fn linux_unmark_preserves_latest_preedit_at_each_atom_seam_once(cx: &mut TestAppContext) {
    for ordinal in 0..=2 {
        let (window, session, node) = open(cx, ordinal);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        window
            .update(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                view.replace_and_mark_text_in_range(None, "nihao🙂", Some(7..7), window, cx);
                let displayed = view.layout_content().0;
                view.unmark_text(window, cx);
                assert_eq!(view.layout_content().0, displayed);
                assert!(!view.is_composing());
                view.unmark_text(window, cx);
                view.cancel_if_composing(cx); // subsequent ordinary focus-out
            })
            .unwrap();
        assert_eq!(text(&session, node), "Anihao🙂中Z");
        assert_eq!(session.borrow().history_depths(), (1, 0));
        let committed = session.borrow().document().clone();
        session.borrow_mut().undo().unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
        session.borrow_mut().redo().unwrap();
        assert_eq!(session.borrow().document().store(), committed.store());
    }
}

#[cfg(target_os = "linux")]
#[gpui::test]
fn linux_unmark_uses_original_nonempty_range_with_unicode_and_reverse_selection(
    cx: &mut TestAppContext,
) {
    for reverse in [false, true] {
        let (window, session, node) = open(cx, 2);
        let inline = session
            .borrow()
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .clone();
        let start = InlinePoint::new(
            node,
            inline.offset_at(1).unwrap(),
            2,
            CursorAffinity::Before,
        );
        let end = InlinePoint::new(
            node,
            inline.offset_at(4).unwrap(),
            0,
            CursorAffinity::Before,
        );
        session
            .borrow_mut()
            .set_inline_selection(
                if reverse { end } else { start },
                if reverse { start } else { end },
            )
            .unwrap();
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        window
            .update(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
                view.replace_and_mark_text_in_range(None, "你好🙂", Some(4..4), window, cx);
                view.unmark_text(window, cx);
                assert_eq!(view.layout_content().0, "A@Ann🙂你好🙂Z");
            })
            .unwrap();
        assert_eq!(text(&session, node), "A你好🙂Z");
        assert_eq!(session.borrow().history_depths(), (1, 0));
        session.borrow_mut().undo().unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
    }
}

#[gpui::test]
fn committed_result_then_unmark_and_focus_out_never_append_preedit(cx: &mut TestAppContext) {
    let (window, session, node) = open(cx, 2);
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            view.replace_text_in_range(None, "你", window, cx);
            view.unmark_text(window, cx);
            view.cancel_if_composing(cx);
            view.unmark_text(window, cx);
        })
        .unwrap();
    assert_eq!(text(&session, node), "A你中Z");
    assert_eq!(session.borrow().history_depths(), (1, 0));
}

#[gpui::test]
fn explicit_empty_cancellation_and_idle_unmark_do_not_write(cx: &mut TestAppContext) {
    for cancel_by_mark in [false, true] {
        let (window, session, _) = open(cx, 1);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        window
            .update(cx, |view, window, cx| {
                view.unmark_text(window, cx);
                view.replace_and_mark_text_in_range(None, "", None, window, cx);
                view.unmark_text(window, cx);
                view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
                if cancel_by_mark {
                    view.replace_and_mark_text_in_range(None, "", None, window, cx);
                } else {
                    view.replace_text_in_range(None, "", window, cx);
                }
                view.unmark_text(window, cx);
                assert!(!view.is_composing());
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn rejected_atom_spanning_composition_cannot_commit_on_unmark(cx: &mut TestAppContext) {
    let (window, session, node) = open(cx, 2);
    let end = InlinePoint::new(
        node,
        session
            .borrow()
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(4)
            .unwrap(),
        0,
        CursorAffinity::Before,
    );
    session
        .borrow_mut()
        .set_inline_selection(InlinePoint::at_start_of(node), end)
        .unwrap();
    let before = session.borrow().document().clone();
    let selection = session.borrow().selection();
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "nihao", None, window, cx);
            assert!(view.rejected_composition);
            view.unmark_text(window, cx);
            assert!(!view.is_composing());
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn empty_mark_then_explicit_result_keeps_selected_range_without_duplicate(cx: &mut TestAppContext) {
    let (window, session, node) = open(cx, 2);
    let inline = session
        .borrow()
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .clone();
    let start = InlinePoint::new(
        node,
        inline.offset_at(1).unwrap(),
        2,
        CursorAffinity::Before,
    );
    let end = InlinePoint::new(
        node,
        inline.offset_at(4).unwrap(),
        0,
        CursorAffinity::Before,
    );
    session
        .borrow_mut()
        .set_inline_selection(start, end)
        .unwrap();
    let before = session.borrow().document().clone();
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            view.replace_and_mark_text_in_range(None, "", None, window, cx);
            view.replace_text_in_range(None, "你", window, cx);
            view.unmark_text(window, cx);
        })
        .unwrap();
    assert_eq!(text(&session, node), "A你Z");
    assert_eq!(session.borrow().history_depths(), (1, 0));
    session.borrow_mut().undo().unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
}

#[gpui::test]
fn focus_out_cancellation_remains_separate_from_unmark(cx: &mut TestAppContext) {
    let (window, session, _) = open(cx, 2);
    let before = session.borrow().document().clone();
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            view.cancel_if_composing(cx);
            view.unmark_text(window, cx);
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[cfg(target_os = "linux")]
#[gpui::test]
fn old_handler_unmarks_before_target_field_changes(cx: &mut TestAppContext) {
    let (old, a, a_node) = open(cx, 0);
    let (target, b, _) = open(cx, 2);
    let before_b = b.borrow().document().clone();
    old.update(cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
        view.unmark_text(window, cx);
        view.cancel_if_composing(cx);
    })
    .unwrap();
    target
        .update(cx, |view, window, cx| view.unmark_text(window, cx))
        .unwrap();
    assert_eq!(text(&a, a_node), "Ani中Z");
    assert_eq!(a.borrow().history_depths(), (1, 0));
    assert_eq!(b.borrow().document().store(), before_b.store());
    assert_eq!(b.borrow().history_depths(), (0, 0));
}

#[cfg(not(target_os = "linux"))]
#[gpui::test]
fn other_platform_bare_unmark_keeps_existing_policy_pending_native_evidence(
    cx: &mut TestAppContext,
) {
    let (window, session, _) = open(cx, 2);
    let before = session.borrow().document().clone();
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            view.unmark_text(window, cx);
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
