use super::*;

fn group(builder: &mut NodeStoreBuilder, kind: NodeKind, children: &[NodeId]) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::children(children.iter().copied()),
        )
        .unwrap()
}

fn step(window: gpui::WindowHandle<DocumentView>, cx: &mut TestAppContext, key: &str) {
    cx.simulate_keystrokes(window.into(), key);
    cx.background_executor.run_until_parked();
}

#[gpui::test]
fn unequal_heights_empty_cells_and_shift_navigation_keep_the_right_column(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let before = paragraph(&mut b, "before");
    let a1 = paragraph(&mut b, "left\nsecond\nthird\nfourth");
    let b1 = paragraph(&mut b, "右🙂");
    let a2 = paragraph(&mut b, "left2\nsecond\nthird");
    let b2 = paragraph(&mut b, "");
    let mut rows = Vec::new();
    for leaves in [[a1, b1], [a2, b2]] {
        let cells: Vec<_> = leaves
            .into_iter()
            .map(|id| group(&mut b, NodeKind::TableCell, &[id]))
            .collect();
        rows.push(group(&mut b, NodeKind::TableRow, &cells));
    }
    let table = group(&mut b, NodeKind::Table, &rows);
    let after = paragraph(&mut b, "after");
    let root = group(&mut b, NodeKind::Document, &[before, table, after]);
    let document = XiaomuDocument::new(root, b.finish()).unwrap();
    let selection = caret_at(&document, b1, 0);
    let (window, session) = open_with(document, selection, cx);
    for (key, expected) in [
        ("down", b2),
        ("down", after),
        ("up", b2),
        ("up", b1),
        ("up", before),
        ("down", b1),
    ] {
        step(window, cx, key);
        assert_eq!(inline_focus(&session).0, expected, "{key}");
    }
    step(window, cx, "shift-down");
    assert_eq!(inline_focus(&session).0, b2);
    assert_eq!(session.borrow().selection().anchor(), selection.anchor());
    assert!(
        session.borrow().selection().active_cell_range().is_none(),
        "ordinary Shift+Down is text selection"
    );
}

#[gpui::test]
fn nested_table_exits_into_its_outer_cell_before_the_next_outer_row(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let a = paragraph(&mut b, "other column\nline2\nline3\nline4");
    let left = group(&mut b, NodeKind::TableCell, &[a]);
    let n1 = paragraph(&mut b, "nested1");
    let n2 = paragraph(&mut b, "nested2");
    let mut nested_rows = Vec::new();
    for leaf in [n1, n2] {
        let cell = group(&mut b, NodeKind::TableCell, &[leaf]);
        nested_rows.push(group(&mut b, NodeKind::TableRow, &[cell]));
    }
    let nested = group(&mut b, NodeKind::Table, &nested_rows);
    let tail = paragraph(&mut b, "after nested");
    let right = group(&mut b, NodeKind::TableCell, &[nested, tail]);
    let row1 = group(&mut b, NodeKind::TableRow, &[left, right]);
    let a2 = paragraph(&mut b, "left2");
    let b2 = paragraph(&mut b, "right2");
    let left2 = group(&mut b, NodeKind::TableCell, &[a2]);
    let right2 = group(&mut b, NodeKind::TableCell, &[b2]);
    let row2 = group(&mut b, NodeKind::TableRow, &[left2, right2]);
    let table = group(&mut b, NodeKind::Table, &[row1, row2]);
    let root = group(&mut b, NodeKind::Document, &[table]);
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let selection = caret_at(&doc, n1, 0);
    let (window, session) = open_with(doc, selection, cx);
    for (key, expected) in [
        ("down", n2),
        ("down", tail),
        ("down", b2),
        ("up", tail),
        ("up", n2),
        ("up", n1),
    ] {
        step(window, cx, key);
        assert_eq!(inline_focus(&session).0, expected, "{key}");
    }
}

#[gpui::test]
fn unicode_wrapped_cell_walks_its_visual_lines_before_changing_rows(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let text = "中文🙂 e\u{301} family👩‍👩‍👧‍👦 ".repeat(16);
    let a1 = paragraph(&mut b, "left");
    let b1 = paragraph(&mut b, &text);
    let a2 = paragraph(&mut b, "left2");
    let b2 = paragraph(&mut b, "end");
    let mut rows = Vec::new();
    for leaves in [[a1, b1], [a2, b2]] {
        let cells: Vec<_> = leaves
            .into_iter()
            .map(|id| group(&mut b, NodeKind::TableCell, &[id]))
            .collect();
        rows.push(group(&mut b, NodeKind::TableRow, &cells));
    }
    let table = group(&mut b, NodeKind::Table, &rows);
    let root = group(&mut b, NodeKind::Document, &[table]);
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let selection = caret_at(&doc, b1, 0);
    let (window, session) = open_with(doc, selection, cx);
    let mut visited = 0;
    for _ in 0..80 {
        step(window, cx, "down");
        let (node, byte) = inline_focus(&session);
        if node == b2 {
            break;
        }
        assert_eq!(node, b1, "must never walk sideways into another column");
        assert!(text.is_char_boundary(byte));
        visited += 1;
    }
    assert!(visited > 1, "fixture must actually wrap");
    assert_eq!(inline_focus(&session).0, b2);
}

#[gpui::test]
fn tab_visits_atomic_only_cell_and_range_input_can_replace_it(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let p = paragraph(&mut b, "left");
    let hr = b
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let c1 = group(&mut b, NodeKind::TableCell, &[p]);
    let c2 = group(&mut b, NodeKind::TableCell, &[hr]);
    let row = group(&mut b, NodeKind::TableRow, &[c1, c2]);
    let table = group(&mut b, NodeKind::Table, &[row]);
    let root = group(&mut b, NodeKind::Document, &[table]);
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let original = doc.clone();
    let selection = caret_at(&doc, p, 0);
    let (window, session) = open_with(doc, selection, cx);
    step(window, cx, "tab");
    assert_eq!(session.borrow().selection().as_atomic_node(), Some(hr));
    step(
        window,
        cx,
        if cfg!(target_os = "macos") {
            "cmd-shift-space"
        } else {
            "ctrl-shift-space"
        },
    );
    assert!(session.borrow().selection().active_cell_range().is_some());
    // simulate_input emits one native insertion per scalar: the first
    // replaces the rectangle, later typing starts a separate coalesced run.
    cx.simulate_input(window.into(), "新");
    cx.background_executor.run_until_parked();
    let node = inline_focus(&session).0;
    assert_eq!(session.borrow().document().parent_of(node), Some(c2));
    step(
        window,
        cx,
        if cfg!(target_os = "macos") {
            "cmd-z"
        } else {
            "ctrl-z"
        },
    );
    assert_eq!(session.borrow().document().store(), original.store());
}
