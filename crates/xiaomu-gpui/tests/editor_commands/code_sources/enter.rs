use super::*;

gpui::actions!(code_source_test, [OuterSubmit]);

#[gpui::test]
fn real_enter_keys_preserve_source_before_code_and_paragraph_mapping(cx: &mut TestAppContext) {
    for kind in [NodeKind::CodeBlock, NodeKind::Paragraph] {
        let (document, node) = single(kind, "");
        let selection = caret(&document, node, 0);
        let router = CodeRouter::new(CodeDecision::Raw);
        let policy = CodePolicy::default();
        let pasted = policy.pasted.clone();
        let editor = EditorInstance::new_with_policy(
            document.clone(),
            selection,
            EditorHooks::default(),
            Box::new(policy),
        )
        .unwrap()
        .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        cx.update(bind_primary_modifier_enter_keys);
        let counts = listen(&session);
        for (key, source, expected) in [
            ("enter", EnterSource::Plain, "plain"),
            ("shift-enter", EnterSource::Shift, "shift"),
            ("ctrl-enter", EnterSource::PrimaryModifier, "primary"),
            ("cmd-enter", EnterSource::PrimaryModifier, "primary"),
        ] {
            press(window, key, cx);
            assert_eq!(text(session.borrow().document(), node), expected);
            assert_eq!(
                children(session.borrow().document(), document.root()),
                [node]
            );
            assert_eq!(session.borrow().history_depths(), (1, 0));
            let observed = router.observed.borrow();
            let call = observed.last().unwrap();
            assert_eq!(call.gesture, CodeGesture::Enter(source));
            assert_eq!(call.document.store(), document.store());
            assert_eq!(call.selection, selection);
            assert_eq!(call.marks, None);
            drop(observed);
            press(window, "ctrl-z", cx);
            assert_eq!(session.borrow().document().store(), document.store());
            assert_eq!(session.borrow().selection(), selection);
        }
        assert_eq!(*pasted.borrow(), ["plain", "shift", "primary", "primary"]);
        assert_eq!(counts.get().0, 8); // Four edits and four undos.
        assert_eq!(router.ordinary.get(), 0);
    }
}

#[gpui::test]
fn old_router_and_no_router_keep_enter_and_shift_defaults(cx: &mut TestAppContext) {
    for configured in [false, true] {
        for kind in [NodeKind::CodeBlock, NodeKind::Paragraph] {
            for key in ["enter", "shift-enter"] {
                let (document, node) = single(kind.clone(), "ab");
                let selection = caret(&document, node, 1);
                let router = Router::new(Decision::Reject); // Implements only the old API.
                let editor =
                    EditorInstance::new(document.clone(), selection, EditorHooks::default())
                        .unwrap();
                let editor = if configured {
                    editor.with_command_router(router.clone())
                } else {
                    editor
                };
                let (window, session) = mount(editor, cx);
                press(window, key, cx);
                let changed = session.borrow().document().clone();
                if kind == NodeKind::CodeBlock || key == "shift-enter" {
                    assert_eq!(children(&changed, changed.root()), [node]);
                    assert_eq!(text(&changed, node), "a\nb");
                } else {
                    let blocks = children(&changed, changed.root());
                    assert_eq!(blocks.len(), 2);
                    assert_eq!(text(&changed, blocks[0]), "a");
                    assert_eq!(text(&changed, blocks[1]), "b");
                }
                assert!(router.observed.borrow().is_empty());
                assert_eq!(session.borrow().history_depths(), (1, 0));
                press(window, "ctrl-z", cx);
                assert_eq!(session.borrow().document().store(), document.store());
                assert_eq!(session.borrow().selection(), selection);
            }
        }
    }
}

#[gpui::test]
fn primary_enter_requires_explicit_binding_then_default_propagates_to_outer_handler(
    cx: &mut TestAppContext,
) {
    let (document, node) = single(NodeKind::CodeBlock, "x");
    let selection = caret(&document, node, 1);
    let router = CodeRouter::new(CodeDecision::Default);
    let editor = EditorInstance::new(document, selection, EditorHooks::default())
        .unwrap()
        .with_command_router(router.clone());
    let outer = Rc::new(Cell::new(0));
    let primary = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let calls = outer.clone();
        cx.on_action(move |_: &OuterSubmit, _| calls.set(calls.get() + 1));
        let calls = primary.clone();
        cx.on_action(move |_: &PrimaryModifierEnter, _| calls.set(calls.get() + 1));
        // Give both actions the same editor scope. An unscoped binding
        // matches the deeper paragraph context in GPUI and intentionally
        // outranks this opt-in, document-scoped binding before dispatch.
        cx.bind_keys([
            gpui::KeyBinding::new("ctrl-enter", OuterSubmit, Some("XiaomuDocument")),
            gpui::KeyBinding::new("cmd-enter", OuterSubmit, Some("XiaomuDocument")),
        ]);
    });
    let (window, session) = mount(editor, cx);
    let counts = listen(&session);
    let before = Snapshot::capture(&session, &counts);
    press(window, "ctrl-enter cmd-enter", cx);
    assert_eq!(outer.get(), 2);
    assert_eq!(primary.get(), 0);
    assert!(router.observed.borrow().is_empty());
    before.assert_unchanged(&session, &counts);

    cx.update(bind_primary_modifier_enter_keys);
    press(window, "ctrl-enter cmd-enter", cx);
    assert_eq!(outer.get(), 2);
    assert_eq!(primary.get(), 2);
    assert_eq!(router.observed.borrow().len(), 2);
    before.assert_unchanged(&session, &counts);

    window
        .update(cx, |view, _, _| view.set_command_router(None))
        .unwrap();
    press(window, "ctrl-enter cmd-enter", cx);
    assert_eq!(primary.get(), 4);
    before.assert_unchanged(&session, &counts);
}
