use super::*;

#[gpui::test]
fn custom_and_unconfigured_instances_are_independent_and_router_can_be_removed(
    cx: &mut TestAppContext,
) {
    let (document, node) = single(NodeKind::Paragraph, "x");
    let selection = caret(&document, node, 1);
    let router = Router::new(Decision::TwoSpaces);
    let custom = EditorInstance::new(document.clone(), selection, EditorHooks::default())
        .unwrap()
        .with_command_router(router.clone());
    let ordinary = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let (a, session_a) = mount(custom, cx);
    let (b, session_b) = mount(ordinary, cx);
    let changes_a = listen(&session_a);
    let changes_b = listen(&session_b);
    let before_b = Snapshot::capture(&session_b, &changes_b);

    focus(a, cx);
    press(a, "ctrl-b", cx);
    let marks = session_a.borrow().stored_marks().cloned();
    press(a, "tab", cx);
    assert_eq!(text(session_a.borrow().document(), node), "x  ");
    assert_eq!(changes_a.get().0, 1);
    before_b.assert_unchanged(&session_b, &changes_b);
    {
        let observed = router.observed.borrow();
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].command, Gesture::Tab(false));
        assert_eq!(observed[0].selection, selection);
        assert_eq!(observed[0].marks, marks);
        assert_eq!(text(&observed[0].document, node), "x");
    }
    let after_tab = Snapshot::capture(&session_a, &changes_a);
    press(a, "shift-tab", cx);
    after_tab.assert_unchanged(&session_a, &changes_a);
    assert_eq!(router.observed.borrow()[1].command, Gesture::Tab(true));

    focus(b, cx);
    press(b, "tab", cx);
    assert_eq!(text(session_b.borrow().document(), node), "x    ");
    after_tab.assert_unchanged(&session_a, &changes_a);
    assert_eq!(session_a.borrow().history_depths(), (1, 0));
    assert_eq!(session_b.borrow().history_depths(), (1, 0));

    a.update(cx, |view, _, _| view.set_command_router(None))
        .unwrap();
    focus(a, cx);
    press(a, "tab", cx);
    assert_eq!(text(session_a.borrow().document(), node), "x      ");
    assert_eq!(router.observed.borrow().len(), 2);
    assert_eq!(text(session_b.borrow().document(), node), "x    ");
    press(a, "ctrl-z", cx);
    assert_eq!(text(session_a.borrow().document(), node), "x  ");
    press(a, "ctrl-z", cx);
    assert_eq!(text(session_a.borrow().document(), node), "x");
    assert_eq!(session_b.borrow().history_depths(), (1, 0));
}

#[gpui::test]
fn default_route_retains_paragraph_four_spaces_and_start_of_block_list_gesture(
    cx: &mut TestAppContext,
) {
    for byte in [0, 1] {
        let (document, node) = single(NodeKind::Paragraph, "x");
        let selection = caret(&document, node, byte);
        let router = Router::new(Decision::Default);
        let editor = EditorInstance::new(document.clone(), selection, EditorHooks::default())
            .unwrap()
            .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        press(window, "tab", cx);
        if byte == 0 {
            let changed = session.borrow().document().clone();
            let list = children(&changed, changed.root())[0];
            assert_eq!(changed.node(list).unwrap().kind(), &NodeKind::BulletList);
            assert_eq!(text(&changed, node), "x");
        } else {
            assert_eq!(text(session.borrow().document(), node), "x    ");
        }
        assert_eq!(session.borrow().history_depths(), (1, 0));
        assert_eq!(router.observed.borrow()[0].command, Gesture::Tab(false));
        press(window, "ctrl-z", cx);
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().selection(), selection);
    }
}

fn list_document(kind: NodeKind) -> (XiaomuDocument, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let a = inline(&mut builder, NodeKind::Paragraph, "a");
    let b = inline(&mut builder, kind, "b");
    let item_a = container(&mut builder, NodeKind::ListItem, &[a]);
    let item_b = container(&mut builder, NodeKind::ListItem, &[b]);
    let list = container(&mut builder, NodeKind::BulletList, &[item_a, item_b]);
    (finish(builder, &[list]), list, item_b, b)
}

#[gpui::test]
fn default_route_keeps_list_indent_outdent_and_code_block_precedence(cx: &mut TestAppContext) {
    for kind in [NodeKind::Paragraph, NodeKind::CodeBlock] {
        let (document, list, item, node) = list_document(kind.clone());
        let selection = caret(&document, node, 0);
        let router = Router::new(Decision::Default);
        let editor = EditorInstance::new(document.clone(), selection, EditorHooks::default())
            .unwrap()
            .with_command_router(router.clone());
        let (window, session) = mount(editor, cx);
        press(window, "tab", cx);
        if kind == NodeKind::CodeBlock {
            assert_eq!(text(session.borrow().document(), node), "    b");
            assert_eq!(session.borrow().document().parent_of(item), Some(list));
            let before = session.borrow().selection();
            press(window, "shift-tab", cx);
            assert_eq!(session.borrow().selection(), before);
            assert_eq!(session.borrow().history_depths(), (1, 0));
            press(window, "ctrl-z", cx);
        } else {
            let changed = session.borrow().document().clone();
            let nested = changed.parent_of(item).unwrap();
            assert_ne!(nested, list);
            assert_eq!(changed.node(nested).unwrap().kind(), &NodeKind::BulletList);
            assert_eq!(children(&changed, list).len(), 1);
            assert_eq!(text(&changed, node), "b");
            press(window, "shift-tab", cx);
            assert_eq!(session.borrow().document().parent_of(item), Some(list));
            assert_eq!(text(session.borrow().document(), node), "b");
            assert_eq!(session.borrow().history_depths(), (2, 0));
            press(window, "ctrl-z ctrl-z", cx);
        }
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().selection(), selection);
        let observed = router.observed.borrow();
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0].command, Gesture::Tab(false));
        assert_eq!(observed[1].command, Gesture::Tab(true));
    }
}

fn table_document() -> (XiaomuDocument, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let a = inline(&mut builder, NodeKind::Paragraph, "a");
    let b = inline(&mut builder, NodeKind::Paragraph, "b");
    let cell_a = container(&mut builder, NodeKind::TableCell, &[a]);
    let cell_b = container(&mut builder, NodeKind::TableCell, &[b]);
    let row = container(&mut builder, NodeKind::TableRow, &[cell_a, cell_b]);
    let table = container(&mut builder, NodeKind::Table, &[row]);
    (finish(builder, &[table]), table, a, b)
}

#[gpui::test]
fn default_route_keeps_table_navigation_and_last_cell_row_creation(cx: &mut TestAppContext) {
    let (document, table, a, b) = table_document();
    let selection = caret(&document, a, 0);
    let router = Router::new(Decision::Default);
    let editor = EditorInstance::new(document.clone(), selection, EditorHooks::default())
        .unwrap()
        .with_command_router(router.clone());
    let (window, session) = mount(editor, cx);
    press(window, "tab", cx);
    assert_eq!(
        session.borrow().selection(),
        caret(session.borrow().document(), b, 0)
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
    press(window, "shift-tab", cx);
    assert_eq!(session.borrow().selection(), selection);
    press(window, "tab tab", cx);
    assert_eq!(children(session.borrow().document(), table).len(), 2);
    assert_eq!(session.borrow().history_depths(), (1, 0));
    assert_eq!(router.observed.borrow().len(), 4);
    press(window, "ctrl-z", cx);
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), caret(&document, b, 0));
}

#[gpui::test]
fn custom_tab_runs_before_paragraph_list_and_table_planning(cx: &mut TestAppContext) {
    let (paragraph, paragraph_node) = single(NodeKind::Paragraph, "x");
    let (list, _, _, list_node) = list_document(NodeKind::Paragraph);
    let (table, _, table_node, _) = table_document();
    for (document, node) in [
        (paragraph, paragraph_node),
        (list, list_node),
        (table, table_node),
    ] {
        let selection = caret(&document, node, 0);
        let node_count = document.node_count();
        let router = Router::new(Decision::TwoSpaces);
        let editor = EditorInstance::new(document.clone(), selection, EditorHooks::default())
            .unwrap()
            .with_command_router(router);
        let (window, session) = mount(editor, cx);
        press(window, "tab", cx);
        assert_eq!(session.borrow().document().node_count(), node_count);
        assert_eq!(
            session.borrow().document().parent_of(node),
            document.parent_of(node)
        );
        assert_eq!(
            text(session.borrow().document(), node),
            format!("  {}", text(&document, node))
        );
        assert_eq!(
            session.borrow().selection(),
            caret(session.borrow().document(), node, 2)
        );
        assert_eq!(session.borrow().history_depths(), (1, 0));
        press(window, "ctrl-z", cx);
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().selection(), selection);
    }
}
