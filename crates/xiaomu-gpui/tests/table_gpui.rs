//! P5.4 Table GPUI contract: real-keystroke cell navigation through the
//! rendered grid, text ↔ table ↔ text visual traversal, in-cell Enter, and
//! click-to-caret inside a cell.

use gpui::{AppContext as _, Modifiers, Point, TestAppContext, px};
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection};

fn offset_of(
    document: &XiaomuDocument,
    node: NodeId,
    byte: usize,
) -> xiaomu_core::text::TextOffset {
    let text: String = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    TextBuffer::from_string(text).offset_at(byte).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

/// `Document > [p("前"), table > [row > [p("a1"), p("b1")]], p("后")]`.
struct Fixture {
    document: XiaomuDocument,
    first: NodeId,
    table: NodeId,
    row1: NodeId,
    a1: NodeId,
    b1: NodeId,
    last: NodeId,
}

fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let first = paragraph(&mut builder, "前");
    let a1 = paragraph(&mut builder, "a1");
    let b1 = paragraph(&mut builder, "b1");
    let last = paragraph(&mut builder, "后");
    let cell_a1 = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([a1]),
        )
        .unwrap();
    let cell_b1 = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([b1]),
        )
        .unwrap();
    let row1 = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell_a1, cell_b1]),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row1]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, table, last]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        table,
        row1,
        a1,
        b1,
        last,
    }
}

fn open_with(
    document: XiaomuDocument,
    selection: DocumentSelection,
    cx: &mut TestAppContext,
) -> (
    gpui::WindowHandle<DocumentView>,
    xiaomu_gpui::block_view::SharedSession,
) {
    let editor = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    (window, session)
}

fn caret_at(document: &XiaomuDocument, node: NodeId, byte: usize) -> DocumentSelection {
    DocumentSelection::collapsed(DocumentPosition::Inline(InlinePoint::new(
        node,
        offset_of(document, node, byte),
        0,
        CursorAffinity::Before,
    )))
}

fn focus_is(session: &xiaomu_gpui::block_view::SharedSession, expected: DocumentPosition) {
    assert_eq!(session.borrow().selection().focus(), expected);
}

fn inline_focus(session: &xiaomu_gpui::block_view::SharedSession) -> (NodeId, usize) {
    match session.borrow().selection().focus() {
        DocumentPosition::Inline(point) => (point.node_id(), point.text_offset().as_usize()),
        other => panic!("expected inline focus, got {other:?}"),
    }
}

#[gpui::test]
fn tab_and_shift_tab_navigate_cells_through_real_keystrokes(cx: &mut TestAppContext) {
    let fixture = fixture();
    let selection = caret_at(&fixture.document, fixture.a1, 2);
    let (window, session) = open_with(fixture.document, selection, cx);
    let step = |window: &gpui::WindowHandle<DocumentView>, cx: &mut TestAppContext, key: &str| {
        cx.simulate_keystrokes((*window).into(), key);
        cx.background_executor.run_until_parked();
    };

    // Tab walks to the next cell in reading order.
    step(&window, cx, "tab");
    focus_is(
        &session,
        DocumentPosition::Inline(InlinePoint::new(
            fixture.b1,
            offset_of(session.borrow().document(), fixture.b1, 0),
            0,
            CursorAffinity::Before,
        )),
    );

    // Shift-Tab walks back; from the first cell it stays put.
    step(&window, cx, "shift-tab");
    focus_is(
        &session,
        DocumentPosition::Inline(InlinePoint::new(
            fixture.a1,
            offset_of(session.borrow().document(), fixture.a1, 0),
            0,
            CursorAffinity::Before,
        )),
    );
    step(&window, cx, "shift-tab");
    focus_is(
        &session,
        DocumentPosition::Inline(InlinePoint::new(
            fixture.a1,
            offset_of(session.borrow().document(), fixture.a1, 0),
            0,
            CursorAffinity::Before,
        )),
    );

    // Tab from the table's last cell appends one trailing row and enters it.
    step(&window, cx, "tab");
    step(&window, cx, "tab");
    let rows: Vec<NodeId> = {
        let document = session.borrow().document().clone();
        document
            .node(fixture.table)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec()
    };
    assert_eq!(rows.len(), 2, "Tab on the last cell appends one row");
    let (node, offset) = inline_focus(&session);
    assert_eq!(offset, 0);
    let new_first_cell = {
        let document = session.borrow().document().clone();
        document
            .node(rows[1])
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec()[0]
    };
    assert_eq!(
        session.borrow().document().parent_of(node),
        Some(new_first_cell),
        "the caret entered the appended row's first cell"
    );

    // Typing lands in the new cell and undo removes exactly it.
    step(&window, cx, "x");
    let text: String = {
        let document = session.borrow().document().clone();
        document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect()
    };
    assert_eq!(text, "x");
    session.borrow_mut().undo().unwrap();
    let text: String = {
        let document = session.borrow().document().clone();
        document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect()
    };
    assert_eq!(text, "");
}

#[gpui::test]
fn up_down_crosses_text_table_text(cx: &mut TestAppContext) {
    let fixture = fixture();
    let selection = caret_at(&fixture.document, fixture.last, 0);
    let (window, session) = open_with(fixture.document, selection, cx);
    let step = |window: &gpui::WindowHandle<DocumentView>, cx: &mut TestAppContext, key: &str| {
        cx.simulate_keystrokes((*window).into(), key);
        cx.background_executor.run_until_parked();
    };

    // Up/Down step one text block in document order (cells participate as
    // ordinary text blocks): 后 → b1 → a1 → 前, and back down.
    step(&window, cx, "up");
    let (node, _) = inline_focus(&session);
    assert_eq!(node, fixture.b1, "up enters the table's last cell");

    step(&window, cx, "up");
    let (node, _) = inline_focus(&session);
    assert_eq!(
        node, fixture.a1,
        "up walks the row's cells in document order"
    );

    step(&window, cx, "up");
    focus_is(
        &session,
        DocumentPosition::Inline(InlinePoint::new(
            fixture.first,
            offset_of(session.borrow().document(), fixture.first, 0),
            0,
            CursorAffinity::Before,
        )),
    );

    step(&window, cx, "down");
    let (node, _) = inline_focus(&session);
    assert_eq!(node, fixture.a1, "down re-enters the table");

    step(&window, cx, "down");
    let (node, _) = inline_focus(&session);
    assert_eq!(node, fixture.b1);

    step(&window, cx, "down");
    focus_is(
        &session,
        DocumentPosition::Inline(InlinePoint::new(
            fixture.last,
            offset_of(session.borrow().document(), fixture.last, 0),
            0,
            CursorAffinity::Before,
        )),
    );
}

#[gpui::test]
fn enter_splits_within_a_cell_and_click_enters_one(cx: &mut TestAppContext) {
    let fixture = fixture();
    let selection = caret_at(&fixture.document, fixture.a1, 1);
    let (window, session) = open_with(fixture.document, selection, cx);
    let step = |window: &gpui::WindowHandle<DocumentView>, cx: &mut TestAppContext, key: &str| {
        cx.simulate_keystrokes((*window).into(), key);
        cx.background_executor.run_until_parked();
    };

    // Enter splits the cell paragraph; the caret lands on the tail inside
    // the same cell.
    step(&window, cx, "enter");
    let (tail, offset) = inline_focus(&session);
    assert_eq!(offset, 0, "the caret sits at the split tail");
    let cell = session.borrow().document().parent_of(fixture.a1).unwrap();
    let blocks: Vec<NodeId> = {
        let document = session.borrow().document().clone();
        document
            .node(cell)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec()
    };
    assert_eq!(
        blocks,
        vec![fixture.a1, tail],
        "the split stays in the cell"
    );
    let _ = fixture.row1;

    // Clicking the first cell's text area places a text caret there. The
    // root padding is 16px, the first text row spans 16..44, the table
    // starts 12px below it and each cell adds 4px vertical padding, so the
    // first cell's text row sits around y=75.
    let mut view = gpui::VisualTestContext::from_window(window.into(), cx);
    view.simulate_click(Point::new(px(48.0), px(75.0)), Modifiers::default());
    cx.background_executor.run_until_parked();
    let (node, _) = inline_focus(&session);
    assert!(
        node == fixture.a1 || node == tail,
        "the click lands in the first cell's text, got {node:?}"
    );
}

fn cell(document: &XiaomuDocument, text: NodeId) -> NodeId {
    document.parent_of(text).unwrap()
}

fn pointer_points(
    window: gpui::WindowHandle<DocumentView>,
    cx: &mut TestAppContext,
) -> [Point<gpui::Pixels>; 2] {
    let width = window
        .update(cx, |_, window, _| f32::from(window.viewport_size().width))
        .unwrap();
    [
        Point::new(px(48.0), px(75.0)),
        Point::new(px(width * 0.75), px(75.0)),
    ]
}

#[gpui::test]
fn clicks_in_both_columns_and_padding_choose_the_containing_cell(cx: &mut TestAppContext) {
    let f = fixture();
    let selection = caret_at(&f.document, f.first, 0);
    let (window, session) = open_with(f.document, selection, cx);
    let points = pointer_points(window, cx);
    let width = window
        .update(cx, |_, window, _| f32::from(window.viewport_size().width))
        .unwrap();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    for (point, node) in [
        (points[1], f.b1),
        (points[0], f.a1),
        (Point::new(px(width / 2.0 + 2.0), px(59.0)), f.b1),
        (Point::new(px(width - 20.0), px(88.0)), f.b1),
    ] {
        visual.simulate_click(point, Modifiers::default());
        assert_eq!(
            inline_focus(&session).0,
            node,
            "incorrect cell at {point:?}"
        );
        session
            .borrow()
            .selection()
            .validate(session.borrow().document())
            .unwrap();
    }
}

#[gpui::test]
fn pointer_range_cut_paste_undo_redo_and_typing_use_native_dispatch(cx: &mut TestAppContext) {
    let f = fixture();
    let cells = [cell(&f.document, f.a1), cell(&f.document, f.b1)];
    let selection = caret_at(&f.document, f.first, 0);
    let (window, session) = open_with(f.document, selection, cx);
    let [left, right] = pointer_points(window, cx);
    {
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(
            left,
            Modifiers {
                alt: true,
                ..Default::default()
            },
        );
        let range = session.borrow().selection().active_cell_range().unwrap();
        assert_eq!((range.anchor(), range.focus()), (cells[0], cells[0]));
        visual.simulate_click(
            right,
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
    }
    let range = session.borrow().selection().active_cell_range().unwrap();
    assert_eq!((range.anchor(), range.focus()), (cells[0], cells[1]));
    cx.simulate_keystrokes(window.into(), "ctrl-x");
    assert_eq!(
        session
            .borrow()
            .clipboard_slice()
            .unwrap()
            .unwrap()
            .plain_text(),
        "\t"
    );
    assert_eq!(session.borrow().history_depths(), (1, 0));
    cx.simulate_keystrokes(window.into(), "ctrl-v");
    assert_eq!(
        session
            .borrow()
            .clipboard_slice()
            .unwrap()
            .unwrap()
            .plain_text(),
        "a1\tb1"
    );
    assert_eq!(session.borrow().history_depths(), (2, 0));
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    assert_eq!(
        session
            .borrow()
            .clipboard_slice()
            .unwrap()
            .unwrap()
            .plain_text(),
        "\t"
    );
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    assert_eq!(
        session
            .borrow()
            .clipboard_slice()
            .unwrap()
            .unwrap()
            .plain_text(),
        "a1\tb1"
    );
    cx.simulate_keystrokes(window.into(), "ctrl-shift-z");
    assert_eq!(
        session
            .borrow()
            .clipboard_slice()
            .unwrap()
            .unwrap()
            .plain_text(),
        "\t"
    );
    cx.simulate_input(window.into(), "中👩‍💻");
    assert!(session.borrow().selection().active_cell_range().is_none());
    let focus = inline_focus(&session).0;
    assert_eq!(session.borrow().document().parent_of(focus), Some(cells[0]));
    let inline = session
        .borrow()
        .document()
        .node(focus)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .clone();
    assert_eq!(
        inline
            .runs()
            .iter()
            .map(|r| r.text().as_str())
            .collect::<String>(),
        "中👩‍💻"
    );
}

#[gpui::test]
fn drag_and_keyboard_extend_rectangles_in_both_dimensions(cx: &mut TestAppContext) {
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let mut f = fixture();
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableRow {
            table: f.table,
            index: 1,
        })
        .apply(&f.document)
        .unwrap();
    let first_cell = cell(&f.document, f.a1);
    let last_row = f
        .document
        .node(f.table)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[1];
    let last_cell = f
        .document
        .node(last_row)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[1];
    let selection = caret_at(&f.document, f.a1, 0);
    let (window, session) = open_with(f.document, selection, cx);
    let [left, right] = pointer_points(window, cx);
    let bottom_right = Point::new(right.x, px(112.0));
    {
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_mouse_down(left, gpui::MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(bottom_right, gpui::MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(bottom_right, gpui::MouseButton::Left, Modifiers::default());
    }
    let range = session.borrow().selection().active_cell_range().unwrap();
    assert_eq!((range.anchor(), range.focus()), (first_cell, last_cell));
    for key in [
        "alt-shift-left",
        "alt-shift-up",
        "alt-shift-right",
        "alt-shift-down",
    ] {
        cx.simulate_keystrokes(window.into(), key);
    }
    assert_eq!(
        session.borrow().selection().active_cell_range(),
        Some(range)
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
    let projection = window
        .update(cx, |view, window, cx| {
            view.accessibility_projection(window, cx).unwrap()
        })
        .unwrap();
    use xiaomu_gpui::accessibility::AccessibilityRole;
    let table = &projection.root().children()[1];
    assert_eq!(table.role(), &AccessibilityRole::Table);
    assert_eq!(table.children()[0].role(), &AccessibilityRole::TableRow);
    assert_eq!(
        table.children()[0].children()[0].role(),
        &AccessibilityRole::TableCell
    );
    assert_eq!(projection.selection().active_cell_range(), Some(range));
}

#[gpui::test]
fn table_editors_isolate_rectangles_focus_and_history(cx: &mut TestAppContext) {
    let a = fixture();
    let b = fixture();
    let a_selection = caret_at(&a.document, a.a1, 0);
    let b_selection = caret_at(&b.document, b.b1, 0);
    let b_store = b.document.store().clone();
    let (window_a, session_a) = open_with(a.document, a_selection, cx);
    let (window_b, session_b) = open_with(b.document, b_selection, cx);
    window_a
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.simulate_keystrokes(window_a.into(), "alt-shift-right");
    assert!(session_a.borrow().selection().active_cell_range().is_some());
    cx.simulate_keystrokes(window_a.into(), "backspace");
    assert_eq!(session_a.borrow().history_depths(), (1, 0));
    assert_eq!(session_b.borrow().selection(), b_selection);
    assert_eq!(session_b.borrow().document().store(), &b_store);
    assert_eq!(session_b.borrow().history_depths(), (0, 0));
    window_b
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.simulate_input(window_b.into(), "乙");
    assert_eq!(session_b.borrow().history_depths(), (1, 0));
    assert_eq!(
        session_a
            .borrow()
            .clipboard_slice()
            .unwrap()
            .unwrap()
            .plain_text(),
        "\t"
    );
    cx.simulate_keystrokes(window_b.into(), "ctrl-z");
    assert_eq!(session_b.borrow().document().store(), &b_store);
    assert_eq!(session_a.borrow().history_depths(), (1, 0));
}

#[gpui::test]
fn multiline_three_column_grid_keeps_padding_clicks_in_each_cell(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let first = paragraph(&mut b, "before");
    let mut rows = Vec::new();
    let mut texts = Vec::new();
    for _ in 0..2 {
        let mut cells = Vec::new();
        for c in 0..3 {
            // Unequal logical-line heights expose y-only hit testing and
            // clicks in the short cell's lower padding region.
            let text = if c == 0 {
                "中文\n👩‍💻\né"
            } else {
                "short"
            };
            let p = paragraph(&mut b, text);
            texts.push(p);
            cells.push(
                b.insert(
                    NodeKind::TableCell,
                    NodeAttrs::empty(),
                    NodeContent::children([p]),
                )
                .unwrap(),
            );
        }
        rows.push(
            b.insert(
                NodeKind::TableRow,
                NodeAttrs::empty(),
                NodeContent::children(cells),
            )
            .unwrap(),
        );
    }
    let table = b
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, table]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let selection = caret_at(&doc, first, 0);
    let (window, session) = open_with(doc, selection, cx);
    let width = window
        .update(cx, |_, window, _| f32::from(window.viewport_size().width))
        .unwrap();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    for row in 0..2 {
        for column in 0..3 {
            for y in [75.0, 130.0] {
                let point = Point::new(
                    px(17.0 + (width - 34.0) * (column as f32 + 0.5) / 3.0),
                    px(y + row as f32 * 93.0),
                );
                visual.simulate_click(point, Modifiers::default());
                assert_eq!(
                    inline_focus(&session).0,
                    texts[row * 3 + column],
                    "point {point:?}"
                );
            }
        }
    }
}
