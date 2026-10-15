//! Positive host navigation and silent input refusal are separate contracts.
use super::*;
use crate::atom_capability::{AtomAction, InlineAtomHostCapability};
use crate::editor_commands::NodeNavigation;
use gpui::{VisualTestContext, point, px};
use xiaomu_core::document::{AtomKind, InlineAtomContent, InlineAtomPlacement};
use xiaomu_core::text::TextOffset;

struct ReadingHost {
    node: NodeId,
    activations: RefCell<Vec<AtomAction>>,
    selections: Cell<usize>,
}
impl InlineAtomHostCapability for ReadingHost {
    fn atom_action(&self, action: AtomAction) {
        self.activations.borrow_mut().push(action);
    }
}
impl EditorCommandRouter for ReadingHost {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        panic!("read-only editing must not invoke the editing router")
    }
    fn select_all(
        &self,
        context: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.selections.set(self.selections.get() + 1);
        Ok(Some(DocumentSelection::all(context.document())))
    }
    fn route_arrow_down(
        &self,
        _: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.selections.set(self.selections.get() + 1);
        Ok(Some(DocumentSelection::collapsed(InlinePoint::new(
            self.node,
            TextOffset::ZERO,
            1,
            CursorAffinity::After,
        ))))
    }
    fn route_node_navigation(
        &self,
        _: EditorCommandContext<'_>,
        _: NodeNavigation,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.selections.set(self.selections.get() + 1);
        Ok(Some(DocumentSelection::collapsed(
            InlinePoint::at_start_of(self.node),
        )))
    }
}

#[gpui::test]
fn read_only_atom_activation_and_selection_routers_remain_available(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("reference").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("reference").unwrap()),
        )
        .unwrap();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("body", MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let host = Rc::new(ReadingHost {
        node,
        activations: RefCell::new(Vec::new()),
        selections: Cell::new(0),
    });
    let editor = EditorInstance::new(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        EditorHooks {
            atom_capability: Some(host.clone()),
            ..Default::default()
        },
    )
    .unwrap()
    .with_command_router(host.clone())
    .with_read_only_input();
    let session = editor.session().clone();
    let handle = open(&editor, cx);
    let position = handle
        .update(cx, |view, _, _| {
            view.block_bounds(node).unwrap().origin + point(px(20.0), px(14.0))
        })
        .unwrap();
    VisualTestContext::from_window(handle.into(), cx).simulate_click(position, Default::default());
    assert_eq!(host.activations.borrow().len(), 1);
    assert_eq!(host.activations.borrow()[0].node, atom);
    cx.simulate_keystrokes(handle.into(), "ctrl-a");
    assert!(
        session
            .borrow()
            .selection()
            .is_all(session.borrow().document())
    );
    cx.simulate_keystrokes(handle.into(), "ctrl-c");
    cx.update(|cx| {
        assert!(
            cx.read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .contains("reference")
        )
    });
    cx.simulate_keystrokes(handle.into(), "down");
    assert_eq!(
        session.borrow().selection(),
        DocumentSelection::collapsed(InlinePoint::new(
            node,
            TextOffset::ZERO,
            1,
            CursorAffinity::After
        ))
    );
    handle
        .update(cx, |view, window, cx| {
            view.select_node(node, window, cx).unwrap();
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "right");
    assert_eq!(
        session.borrow().selection(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(node))
    );
    assert_eq!(host.selections.get(), 3);
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn read_only_refusals_are_silent_before_clock_or_rejection_events(cx: &mut TestAppContext) {
    let (document, blocks) = document(1);
    let calls = Rc::new(Calls::default());
    let editor = instance(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        true,
        calls.clone(),
    );
    let handle = open(&editor, cx);
    let events = Rc::new(Cell::new(0));
    let observed = events.clone();
    let _subscription = handle
        .update(cx, |_, _, cx| {
            let entity = cx.entity();
            cx.subscribe(&entity, move |_, _, _: &EditorRejection, _| {
                observed.set(observed.get() + 1)
            })
        })
        .unwrap();
    handle
        .update(cx, |view, window, cx| {
            exercise_native(&view.children[0].1.clone(), window, cx);
            view.apply_edit_intent(EditIntent::ToggleMark { mark: Mark::Bold }, window, cx);
            view.cut(&ClipboardCut, window, cx);
            cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(
                "fallback".into(),
                "xiaomu.clipboard.v14\ninvalid".into(),
            ));
            view.paste(&ClipboardPaste, window, cx);
        })
        .unwrap();
    for key in ["ctrl-v", "ctrl-x", "enter", "ctrl-z"] {
        cx.simulate_keystrokes(handle.into(), key);
    }
    cx.background_executor.run_until_parked();
    assert_eq!(events.get(), 0);
    assert_eq!(calls.clock.get(), 0);
    assert_eq!(calls.intent.get(), 0);
}
