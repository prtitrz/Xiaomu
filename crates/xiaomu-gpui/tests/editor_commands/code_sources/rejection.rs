use super::*;

#[gpui::test]
fn rejected_code_hooks_preserve_every_state_and_typing_group(cx: &mut TestAppContext) {
    for decision in [
        CodeDecision::NoChange,
        CodeDecision::Reject,
        CodeDecision::RejectedCandidate,
    ] {
        let (document, node) = single(NodeKind::CodeBlock, "");
        let selection = caret(&document, node, 0);
        let router = CodeRouter::new(decision);
        let editor = EditorInstance::new_with_policy(
            document.clone(),
            selection,
            EditorHooks::default(),
            Box::<CodePolicy>::default(),
        )
        .unwrap()
        .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        cx.update(bind_primary_modifier_enter_keys);
        let counts = listen(&session);
        press(window, "ctrl-b", cx);
        cx.simulate_input(window.into(), "a");
        let before = Snapshot::capture(&session, &counts);
        assert_eq!(before.marks, Some(MarkSet::new([Mark::Bold]).unwrap()));
        for key in ["enter", "shift-enter", "ctrl-enter", "cmd-enter"] {
            press(window, key, cx);
            before.assert_unchanged(&session, &counts);
        }
        paste_text(window, "raw\r\ntext", cx);
        before.assert_unchanged(&session, &counts);
        for closed in [false, true] {
            write_slice(&raw_slice("raw\rtext", closed), cx);
            press(window, "ctrl-v", cx);
            before.assert_unchanged(&session, &counts);
        }
        assert_eq!(router.observed.borrow().len(), 7);
        assert_eq!(router.ordinary.get(), 0);
        for call in router.observed.borrow().iter() {
            assert_eq!(call.document.store(), before.document.store());
            assert_eq!(call.selection, before.selection);
            assert_eq!(call.marks, before.marks);
        }
        cx.simulate_input(window.into(), "b");
        assert_eq!(text(session.borrow().document(), node), "ab");
        assert_eq!(session.borrow().history_depths(), (1, 0));
        press(window, "ctrl-z", cx);
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().selection(), selection);
        press(window, "ctrl-shift-z", cx);
        assert_eq!(text(session.borrow().document(), node), "ab");
    }
}

#[gpui::test]
fn rejected_code_hooks_preserve_backward_selection_and_redo(cx: &mut TestAppContext) {
    for decision in [
        CodeDecision::NoChange,
        CodeDecision::Reject,
        CodeDecision::RejectedCandidate,
    ] {
        let (document, node) = single(NodeKind::CodeBlock, "abc");
        let selection =
            DocumentSelection::new(point(&document, node, 3), point(&document, node, 1));
        let router = CodeRouter::new(decision);
        let editor = EditorInstance::new_with_policy(
            document.clone(),
            selection,
            EditorHooks::default(),
            Box::<CodePolicy>::default(),
        )
        .unwrap()
        .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        cx.update(bind_primary_modifier_enter_keys);
        let counts = listen(&session);
        cx.simulate_input(window.into(), "z");
        press(window, "ctrl-z", cx);
        let before = Snapshot::capture(&session, &counts);
        assert_eq!(before.history, (0, 1));
        assert_eq!(before.selection, selection);
        press(window, "enter shift-enter ctrl-enter cmd-enter", cx);
        paste_text(window, "raw\r\ntext", cx);
        for closed in [false, true] {
            write_slice(&raw_slice("raw\rtext", closed), cx);
            press(window, "ctrl-v", cx);
        }
        before.assert_unchanged(&session, &counts);
        press(window, "ctrl-shift-z", cx);
        assert_eq!(text(session.borrow().document(), node), "az");
    }
}
