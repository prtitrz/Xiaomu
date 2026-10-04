use super::*;

#[gpui::test]
fn raw_code_paste_preserves_line_endings_source_policy_and_single_undo(cx: &mut TestAppContext) {
    let raw = "甲\r\n乙\r丙\n丁\t末";
    for source in [
        CodePasteSource::PlatformText,
        CodePasteSource::NativeStructuredPlainText { closed: false },
        CodePasteSource::NativeStructuredPlainText { closed: true },
    ] {
        let (document, node) = single(NodeKind::CodeBlock, "");
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
        let counts = listen(&session);
        match source {
            CodePasteSource::PlatformText => paste_text(window, raw, cx),
            CodePasteSource::NativeStructuredPlainText { closed } => {
                write_slice(&raw_slice(raw, closed), cx);
                press(window, "ctrl-v", cx);
            }
        }
        let changed = session.borrow().document().clone();
        assert_eq!(text(&changed, node), raw);
        assert_eq!(children(&changed, changed.root()), [node]);
        assert_eq!(session.borrow().history_depths(), (1, 0));
        assert_eq!(counts.get().0, 1);
        assert_eq!(*pasted.borrow(), [raw]);
        let observed = router.observed.borrow();
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].gesture, CodeGesture::Paste(raw.into(), source));
        assert_eq!(observed[0].document.store(), document.store());
        assert_eq!(observed[0].selection, selection);
        assert_eq!(router.ordinary.get(), 0);
        drop(observed);
        press(window, "ctrl-z", cx);
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().selection(), selection);
        press(window, "ctrl-shift-z", cx);
        assert_eq!(session.borrow().document().store(), changed.store());
        assert_eq!(router.observed.borrow().len(), 1);
    }
}

#[gpui::test]
fn default_code_routes_normalize_only_text_and_open_slices_keep_closed_failure(
    cx: &mut TestAppContext,
) {
    let raw = "a\r\nb\rc\nd";
    // None, old exhaustive router, and explicit Default must agree.
    for mode in 0..3 {
        let (document, node) = single(NodeKind::CodeBlock, "");
        let selection = caret(&document, node, 0);
        let old = Router::new(Decision::Reject);
        let router = CodeRouter::new(CodeDecision::Default);
        let editor =
            EditorInstance::new(document.clone(), selection, EditorHooks::default()).unwrap();
        let editor = match mode {
            1 => editor.with_command_router(old.clone()),
            2 => editor.with_command_router(router.clone()),
            _ => editor,
        };
        let (window, session) = mount(editor, cx);
        let counts = listen(&session);
        paste_text(window, raw, cx);
        assert_eq!(text(session.borrow().document(), node), "a\nb\nc\nd");
        press(window, "ctrl-z", cx);
        write_slice(&raw_slice(raw, false), cx);
        press(window, "ctrl-v", cx);
        assert_eq!(text(session.borrow().document(), node), "a\nb\nc\nd");
        press(window, "ctrl-z", cx);
        let before = Snapshot::capture(&session, &counts);
        write_slice(&raw_slice(raw, true), cx);
        press(window, "ctrl-v", cx);
        before.assert_unchanged(&session, &counts);
        assert!(old.observed.borrow().is_empty());
        assert_eq!(router.ordinary.get(), 0);
        if mode == 2 {
            let observed = router.observed.borrow();
            assert_eq!(observed.len(), 3);
            assert_eq!(
                observed[0].gesture,
                CodeGesture::Paste(raw.into(), CodePasteSource::PlatformText)
            );
            assert_eq!(
                observed[1].gesture,
                CodeGesture::Paste(
                    raw.into(),
                    CodePasteSource::NativeStructuredPlainText { closed: false }
                )
            );
            assert_eq!(
                observed[2].gesture,
                CodeGesture::Paste(
                    raw.into(),
                    CodePasteSource::NativeStructuredPlainText { closed: true }
                )
            );
        }
    }
}

#[gpui::test]
fn code_hook_excludes_images_plain_target_and_ordinary_input(cx: &mut TestAppContext) {
    for kind in [NodeKind::CodeBlock, NodeKind::Paragraph] {
        let (document, node) = single(kind.clone(), "");
        let selection = caret(&document, node, 0);
        let router = CodeRouter::new(CodeDecision::NoChange);
        let editor = EditorInstance::new(document, selection, EditorHooks::default())
            .unwrap()
            .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        cx.simulate_input(window.into(), "x");
        assert_eq!(text(session.borrow().document(), node), "x");
        cx.update(|cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
                gpui::ImageFormat::Png,
                vec![1, 2, 3],
            )));
        });
        let counts = listen(&session);
        let before = Snapshot::capture(&session, &counts);
        press(window, "ctrl-v", cx);
        before.assert_unchanged(&session, &counts);
        assert!(router.observed.borrow().is_empty());
        assert_eq!(router.ordinary.get(), 0);
        if kind == NodeKind::Paragraph {
            write_slice(&raw_slice("native", false), cx);
            press(window, "ctrl-v", cx);
            assert_eq!(text(session.borrow().document(), node), "xnative");
            assert_eq!(router.ordinary.get(), 0);
            assert!(router.observed.borrow().is_empty());
            paste_text(window, "p\rq", cx);
            assert_eq!(router.ordinary.get(), 1);
            assert!(router.observed.borrow().is_empty());
        }
    }
}
