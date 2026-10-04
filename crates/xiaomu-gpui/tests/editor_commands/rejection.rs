use super::*;
use xiaomu_runtime::session::SessionPolicy;

struct RejectExclamation;

impl SessionPolicy for RejectExclamation {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document
            .store()
            .iter()
            .filter_map(|node| node.content().as_inline())
            .flat_map(|inline| inline.runs())
            .any(|run| run.text().as_str().contains('!'))
        {
            return Err(PolicyError::new("candidate rejected"));
        }
        Ok(())
    }
}

#[gpui::test]
fn rejected_and_no_change_routes_preserve_all_state_and_typing_group(cx: &mut TestAppContext) {
    for decision in [
        Decision::Reject,
        Decision::NoChange,
        Decision::RejectedCandidate,
    ] {
        let (document, node) = single(NodeKind::Paragraph, "");
        let selection = caret(&document, node, 0);
        let router = Router::new(decision);
        let editor = EditorInstance::new_with_policy(
            document.clone(),
            selection,
            EditorHooks::default(),
            Box::new(RejectExclamation),
        )
        .unwrap()
        .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        let counts = listen(&session);
        press(window, "ctrl-b", cx);
        cx.simulate_input(window.into(), "a");
        assert_eq!(text(session.borrow().document(), node), "a");
        assert_eq!(
            session.borrow().stored_marks(),
            Some(&MarkSet::new([Mark::Bold]).unwrap())
        );
        let before = Snapshot::capture(&session, &counts);
        press(window, "tab", cx);
        before.assert_unchanged(&session, &counts);
        press(window, "shift-tab", cx);
        before.assert_unchanged(&session, &counts);
        paste_text(window, "raw\r\ntext", cx);
        before.assert_unchanged(&session, &counts);
        {
            let observed = router.observed.borrow();
            assert_eq!(observed.len(), 3);
            assert_eq!(observed[0].command, Gesture::Tab(false));
            assert_eq!(observed[1].command, Gesture::Tab(true));
            assert_eq!(observed[2].command, Gesture::Paste("raw\r\ntext".into()));
            for call in observed.iter() {
                assert_eq!(call.document.store(), before.document.store());
                assert_eq!(call.selection, before.selection);
                assert_eq!(call.marks, before.marks);
            }
        }
        cx.simulate_input(window.into(), "b");
        assert_eq!(text(session.borrow().document(), node), "ab");
        assert_eq!(
            session.borrow().history_depths(),
            (1, 0),
            "consumed commands must not split typing"
        );
        assert_eq!(counts.get().0, 2);
        press(window, "ctrl-z", cx);
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 1));
        press(window, "ctrl-shift-z", cx);
        assert_eq!(text(session.borrow().document(), node), "ab");
    }
}

#[gpui::test]
fn rejected_routes_preserve_backward_range_and_redo_history(cx: &mut TestAppContext) {
    for decision in [
        Decision::Reject,
        Decision::NoChange,
        Decision::RejectedCandidate,
    ] {
        let (document, node) = single(NodeKind::Paragraph, "abc");
        let anchor = InlinePoint::new(
            node,
            document
                .node(node)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .offset_at(3)
                .unwrap(),
            0,
            CursorAffinity::After,
        );
        let selection = DocumentSelection::new(anchor, point(&document, node, 1));
        let router = Router::new(decision);
        let editor = EditorInstance::new_with_policy(
            document.clone(),
            selection,
            EditorHooks::default(),
            Box::new(RejectExclamation),
        )
        .unwrap()
        .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        let counts = listen(&session);
        // Prime a redo branch and return to the original backward selection.
        cx.simulate_input(window.into(), "z");
        press(window, "ctrl-z", cx);
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 1));
        let before = Snapshot::capture(&session, &counts);
        press(window, "tab shift-tab", cx);
        paste_text(window, "a\nb", cx);
        before.assert_unchanged(&session, &counts);
        for call in router.observed.borrow().iter() {
            assert_eq!(call.selection, selection);
            assert_eq!(call.document.store(), document.store());
        }
        press(window, "ctrl-shift-z", cx);
        assert_eq!(text(session.borrow().document(), node), "az");
    }
}
