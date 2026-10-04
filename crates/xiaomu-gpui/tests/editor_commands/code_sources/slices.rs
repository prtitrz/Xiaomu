use super::*;

struct SliceRouter {
    slices: RefCell<Vec<(ClipboardSlice, DocumentSelection)>>,
    raw: RefCell<Vec<(String, CodePasteSource)>>,
}
impl EditorCommandRouter for SliceRouter {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        panic!("native code paste must not enter the ordinary text hook")
    }
    fn route_code_paste(
        &self,
        _: EditorCommandContext<'_>,
        raw: &str,
        source: CodePasteSource,
    ) -> Result<CommandRoute, PolicyError> {
        self.raw.borrow_mut().push((raw.into(), source));
        Ok(CommandRoute::NoChange)
    }
    fn route_code_slice(
        &self,
        context: EditorCommandContext<'_>,
        slice: &ClipboardSlice,
    ) -> Result<CommandRoute, PolicyError> {
        self.slices
            .borrow_mut()
            .push((slice.clone(), context.selection()));
        // A host can choose a block separator using the validated fragments,
        // without changing either the native codec or foreign platform text.
        let text = slice
            .blocks()
            .iter()
            .map(|block| block.inline().plain_text())
            .collect::<Vec<_>>()
            .join("\n\n");
        Ok(CommandRoute::Intent(EditIntent::PasteText { text }))
    }
}

#[gpui::test]
fn code_slice_override_receives_validated_tree_while_platform_text_stays_raw(
    cx: &mut TestAppContext,
) {
    for closed in [false, true] {
        let (document, node) = single(NodeKind::CodeBlock, "");
        let selection = caret(&document, node, 0);
        let router = Rc::new(SliceRouter {
            slices: RefCell::new(Vec::new()),
            raw: RefCell::new(Vec::new()),
        });
        let policy = CodePolicy::default();
        let observed_intents = policy.pasted.clone();
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
        let slice = if closed {
            let mut builder = NodeStoreBuilder::new();
            let a = inline(&mut builder, NodeKind::Paragraph, "甲");
            let b = inline(&mut builder, NodeKind::Paragraph, "乙");
            let doc = finish(builder, &[a, b]);
            let selected = DocumentSelection::all(&doc);
            DocumentSession::new(doc, selected)
                .unwrap()
                .clipboard_slice()
                .unwrap()
                .unwrap()
        } else {
            multiline_slice("甲\n乙")
        };
        assert_eq!(slice.is_closed(), closed);
        assert_eq!(slice.plain_text(), "甲\n乙");
        write_slice(&slice, cx);
        press(window, "ctrl-v", cx);
        assert_eq!(*router.slices.borrow(), [(slice, selection)]);
        assert!(router.raw.borrow().is_empty());
        assert_eq!(*observed_intents.borrow(), ["甲\n\n乙"]);
        assert_eq!(text(session.borrow().document(), node), "甲\n\n乙");
        assert_eq!(session.borrow().history_depths(), (1, 0));
        assert_eq!(counts.get().0, 1);
        press(window, "ctrl-z", cx);
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().selection(), selection);
        let before = Snapshot::capture(&session, &counts);
        let raw = "甲\r\n乙\r丙";
        paste_text(window, raw, cx);
        assert_eq!(
            *router.raw.borrow(),
            [(raw.into(), CodePasteSource::PlatformText)]
        );
        assert_eq!(router.slices.borrow().len(), 1);
        before.assert_unchanged(&session, &counts);
    }
}
