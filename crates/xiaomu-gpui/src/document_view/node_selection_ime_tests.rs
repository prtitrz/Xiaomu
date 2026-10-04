//! Real input-handler lifecycle over a tagged, empty native range surface.

use super::*;

fn policy(seen: &Seen, reject_prepare: bool, reject_final: bool) -> Box<dyn SessionPolicy> {
    Box::new(ReplaceNode {
        seen: seen.clone(),
        reject_prepare,
        reject_final,
    })
}

#[gpui::test]
fn node_preedit_cancel_preserves_tag_and_successful_commit_is_one_undo_unit(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (handle, session, counts) = open(cx, &f, Some(policy(&seen, false, false)));
    select(cx, handle, f.quote);
    let selection = session.borrow().selection();
    counts.set((0, 0));
    let input = proxy(cx, handle);
    handle
        .update(cx, |view, window, cx| {
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(Some(15..70), "nihao", Some(2..2), window, cx);
                assert_eq!(input.marked_text_range(window, cx), Some(0..5));
                assert_eq!(
                    input.selected_text_range(false, window, cx).unwrap().range,
                    2..2
                );
                assert_eq!(input.display_content().0, "nihao");
            });
            assert_eq!(view.select_node(f.intro, window, cx).unwrap(), None);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_unchanged(&session, &f, selection, &counts);
    assert!(seen.borrow().is_empty());
    let painted = bounds(cx, handle, "node-selection-input", f.quote);
    handle
        .update(cx, |_, window, cx| {
            input.update(cx, |input, cx| {
                assert_eq!(
                    input
                        .bounds_for_range(0..5, painted, window, cx)
                        .unwrap()
                        .top(),
                    painted.top()
                );
                input.replace_and_mark_text_in_range(None, "", None, window, cx);
                input.unmark_text(window, cx);
                assert!(!input.is_composing());
                assert_eq!(input.marked_text_range(window, cx), None);
                assert_eq!(input.display_content().0, "");
            });
        })
        .unwrap();
    assert_unchanged(&session, &f, selection, &counts);
    handle
        .update(cx, |_, window, cx| {
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "你好🙂", Some(4..4), window, cx);
                assert_eq!(input.marked_text_range(window, cx), Some(0..4));
                input.replace_text_in_range(Some(0..4), "你好🙂", window, cx);
                input.unmark_text(window, cx);
                input.unmark_text(window, cx);
                assert!(!input.is_composing());
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(session.borrow().history_depths(), (1, 0));
    assert_eq!(counts.get().0, 1);
    assert!(session.borrow().document().node(f.quote).is_none());
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(seen.borrow()[0].0, selection);
    assert!(
        matches!(&seen.borrow()[0].1, EditIntent::CommitComposition { range, text } if range.start().as_usize() == 0 && range.end().as_usize() == 0 && text == "你好🙂")
    );
    handle
        .update(cx, |view, window, cx| {
            assert!(view.range_input.is_none());
            let DocumentPosition::Inline(at) = session.borrow().selection().focus() else {
                panic!("replacement caret");
            };
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == at.node_id())
                .unwrap()
                .1
                .clone();
            assert!(child.read(cx).focus_handle(cx).is_focused(window));
            assert_eq!(child.read(cx).canonical_text(), "你好🙂");
        })
        .unwrap();
    key(cx, handle, "ctrl-z");
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().document().store(), f.document.store());
    assert_eq!(session.borrow().history_depths(), (0, 1));
    handle
        .update(cx, |view, window, cx| {
            assert!(view.range_input_is_focused(window, cx))
        })
        .unwrap();
    key(cx, handle, "ctrl-y");
    assert_eq!(session.borrow().history_depths(), (1, 0));
    assert!(session.borrow().selection().as_node_selection().is_none());
}

#[gpui::test]
fn rejected_node_commit_clears_only_preedit_without_listener_or_undo_pollution(
    cx: &mut TestAppContext,
) {
    for reject_prepare in [true, false] {
        let f = fixture();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let (handle, session, counts) = open(cx, &f, Some(policy(&seen, reject_prepare, true)));
        select(cx, handle, f.quote);
        let selection = session.borrow().selection();
        counts.set((0, 0));
        let input = proxy(cx, handle);
        handle
            .update(cx, |_, window, cx| {
                input.update(cx, |input, cx| {
                    input.replace_and_mark_text_in_range(None, "blocked", None, window, cx);
                    input.replace_text_in_range(None, "blocked", window, cx);
                    input.unmark_text(window, cx);
                    input.unmark_text(window, cx);
                    assert!(!input.is_composing());
                    assert_eq!(input.display_content().0, "");
                    assert_eq!(
                        input.selected_text_range(false, window, cx).unwrap().range,
                        0..0
                    );
                });
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        assert_unchanged(&session, &f, selection, &counts);
        assert_eq!(seen.borrow().len(), 1);
        assert_eq!(seen.borrow()[0].0, selection);
        handle
            .update(cx, |view, window, cx| {
                assert!(view.range_input_is_focused(window, cx))
            })
            .unwrap();
    }
}

#[cfg(target_os = "linux")]
#[gpui::test]
fn linux_node_unmark_commits_once_and_failed_unmark_preserves_selection(cx: &mut TestAppContext) {
    for reject in [false, true] {
        let f = fixture();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let (handle, session, counts) = open(cx, &f, Some(policy(&seen, reject, false)));
        select(cx, handle, f.rule);
        let selection = session.borrow().selection();
        counts.set((0, 0));
        let input = proxy(cx, handle);
        handle
            .update(cx, |_, window, cx| {
                input.update(cx, |input, cx| {
                    input.replace_and_mark_text_in_range(None, "你好🙂", Some(4..4), window, cx);
                    input.unmark_text(window, cx);
                    input.unmark_text(window, cx);
                    assert!(!input.is_composing());
                });
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        assert_eq!(seen.borrow().len(), 1);
        assert_eq!(seen.borrow()[0].0, selection);
        if reject {
            assert_unchanged(&session, &f, selection, &counts);
        } else {
            assert_eq!(counts.get().0, 1);
            assert_eq!(session.borrow().history_depths(), (1, 0));
            key(cx, handle, "ctrl-z");
            assert_eq!(session.borrow().selection(), selection);
            assert_eq!(session.borrow().document().store(), f.document.store());
        }
    }
}

#[gpui::test]
fn node_plain_input_ignores_platform_range_and_keeps_original_policy_selection(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (handle, session, counts) = open(cx, &f, Some(policy(&seen, false, false)));
    select(cx, handle, f.quote);
    let selection = session.borrow().selection();
    counts.set((0, 0));
    let input = proxy(cx, handle);
    handle
        .update(cx, |_, window, cx| {
            input.update(cx, |input, cx| {
                input.replace_text_in_range(Some(9..99), "replacement", window, cx);
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(seen.borrow()[0].0, selection);
    assert!(
        matches!(&seen.borrow()[0].1, EditIntent::InsertText { text } if text == "replacement")
    );
    assert_eq!(session.borrow().history_depths(), (1, 0));
    assert_eq!(counts.get().0, 1);
    assert!(session.borrow().document().node(f.quote).is_none());
    cx.simulate_input(handle.into(), "!");
    cx.background_executor.run_until_parked();
    let DocumentPosition::Inline(at) = session.borrow().selection().focus() else {
        panic!("replacement caret");
    };
    assert_eq!(
        session
            .borrow()
            .document()
            .node(at.node_id())
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect::<String>(),
        "replacement!"
    );
}

#[gpui::test]
fn node_proxy_never_projects_real_inline_atoms_into_virtual_preedit(cx: &mut TestAppContext) {
    use xiaomu_core::document::{AtomKind, InlineAtomContent};
    let mut f = fixture();
    f.document = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::at_start_of(f.intro),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("@ann").unwrap(),
        })
        .apply(&f.document)
        .unwrap();
    let (handle, _, _) = open(cx, &f, None);
    select(cx, handle, f.intro);
    let input = proxy(cx, handle);
    handle
        .update(cx, |_, window, cx| {
            input.update(cx, |input, cx| {
                assert!(input.atom_display_projection().is_none());
                assert_eq!(input.layout_content().0, "");
                input.replace_and_mark_text_in_range(None, "你", None, window, cx);
                assert_eq!(input.layout_content().0, "你");
                assert_eq!(input.display_content().0, "你");
                input.replace_and_mark_text_in_range(None, "", None, window, cx);
            })
        })
        .unwrap();
}
