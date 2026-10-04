use super::*;

struct ArrowRouter {
    target: Result<Option<DocumentSelection>, PolicyError>,
    observed: RefCell<Vec<DocumentSelection>>,
}

impl EditorCommandRouter for ArrowRouter {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }

    fn route_arrow_down(
        &self,
        context: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.observed.borrow_mut().push(context.selection());
        self.target.clone()
    }
}

fn code_and_tail() -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let code = inline(&mut builder, NodeKind::CodeBlock, "a");
    let tail = inline(&mut builder, NodeKind::Paragraph, "tail");
    (finish(builder, &[code, tail]), code, tail)
}

#[gpui::test]
fn arrow_down_hook_moves_only_selection_and_routes_native_focus(cx: &mut TestAppContext) {
    let (document, code, tail) = code_and_tail();
    let original = caret(&document, code, 1);
    let target = caret(&document, tail, 0);
    let router = Rc::new(ArrowRouter {
        target: Ok(Some(target)),
        observed: RefCell::new(Vec::new()),
    });
    let editor = EditorInstance::new(document.clone(), original, EditorHooks::default())
        .unwrap()
        .with_command_router(router.clone());
    let (window, session) = mount(editor, cx);
    press(window, "ctrl-b", cx);
    let counts = listen(&session);
    let before = Snapshot::capture(&session, &counts);
    press(window, "down", cx);
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().document().revision(), document.revision());
    assert_eq!(session.borrow().selection(), target);
    assert_eq!(session.borrow().history_depths(), before.history);
    assert_eq!(session.borrow().stored_marks(), None);
    assert_eq!(counts.get(), (0, 1));
    assert_eq!(*router.observed.borrow(), [original]);
    // Input follows the moved native focus and does not edit the old code block.
    cx.simulate_input(window.into(), "x");
    assert_eq!(text(session.borrow().document(), tail), "xtail");
    assert_eq!(text(session.borrow().document(), code), "a");
    assert_eq!(session.borrow().history_depths(), (1, 0));
    press(window, "ctrl-z", cx);
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), target);
}

#[gpui::test]
fn arrow_down_default_matches_old_router_and_shift_down_skips_hook(cx: &mut TestAppContext) {
    let (document, code, _) = code_and_tail();
    let original = caret(&document, code, 1);
    let mut default_selection = None;
    for mode in 0..3 {
        let router = Rc::new(ArrowRouter {
            target: Ok(None),
            observed: RefCell::new(Vec::new()),
        });
        let editor =
            EditorInstance::new(document.clone(), original, EditorHooks::default()).unwrap();
        let editor = match mode {
            1 => editor.with_command_router(Router::new(Decision::Reject)),
            2 => editor.with_command_router(router.clone()),
            _ => editor,
        };
        let (window, session) = mount(editor, cx);
        press(window, "down", cx);
        let selection = session.borrow().selection();
        if let Some(expected) = default_selection {
            assert_eq!(selection, expected);
        } else {
            assert_ne!(selection, original, "baseline Down must really navigate");
            default_selection = Some(selection);
        }
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
        assert_eq!(router.observed.borrow().len(), usize::from(mode == 2));
        press(window, "shift-down", cx);
        assert_eq!(router.observed.borrow().len(), usize::from(mode == 2));
    }
}

#[gpui::test]
fn arrow_down_rejection_invalid_and_unchanged_targets_preserve_all_state(cx: &mut TestAppContext) {
    let (document, code, _) = code_and_tail();
    let original = caret(&document, code, 1);
    let (long_document, long_node) = single(NodeKind::CodeBlock, "longer");
    assert_eq!(long_node, code);
    let invalid = caret(&long_document, long_node, 5);
    for target in [
        Err(PolicyError::new("rejected")),
        Ok(Some(invalid)),
        Ok(Some(original)),
    ] {
        let router = Rc::new(ArrowRouter {
            target,
            observed: RefCell::new(Vec::new()),
        });
        let editor = EditorInstance::new(document.clone(), original, EditorHooks::default())
            .unwrap()
            .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        press(window, "ctrl-b", cx);
        cx.simulate_input(window.into(), "z");
        press(window, "ctrl-z", cx);
        let counts = listen(&session);
        let before = Snapshot::capture(&session, &counts);
        assert_eq!(before.history, (0, 1));
        press(window, "down", cx);
        before.assert_unchanged(&session, &counts);
        assert_eq!(*router.observed.borrow(), [original]);
        press(window, "ctrl-shift-z", cx);
        assert_eq!(text(session.borrow().document(), code), "az");
    }
}
