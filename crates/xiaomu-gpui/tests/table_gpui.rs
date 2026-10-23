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

#[path = "table/vertical.rs"]
mod vertical;

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
            NodeContent::Inline(if text.is_empty() {
                InlineContent::empty()
            } else {
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
            }),
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

    // Up/Down preserves the visual column: 后 → a1 → 前 and back.
    // A same-row neighbor belongs to Tab/Left/Right, not vertical movement.
    step(&window, cx, "up");
    let (node, _) = inline_focus(&session);
    assert_eq!(node, fixture.a1, "up enters the visually aligned cell");

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

#[gpui::test]
fn clicking_right_cell_enters_right_cell(cx: &mut TestAppContext) {
    let fixture = fixture();
    let selection = caret_at(&fixture.document, fixture.first, 0);
    let (window, session) = open_with(fixture.document, selection, cx);
    let width = window
        .update(cx, |_, window, _| window.viewport_size().width)
        .unwrap();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(Point::new(width * 0.75, px(75.0)), Modifiers::default());
    cx.background_executor.run_until_parked();
    assert_eq!(
        inline_focus(&session).0,
        fixture.b1,
        "a click in the right cell must not enter the left cell"
    );
}

#[gpui::test]
fn clicking_blank_space_in_a_short_cell_stays_in_that_cell(cx: &mut TestAppContext) {
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let fixture = fixture();
    let left_cell = fixture.document.parent_of(fixture.a1).unwrap();
    let document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertNode {
            parent: left_cell,
            index: 1,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        })
        .apply(&fixture.document)
        .unwrap();
    let selection = caret_at(&document, fixture.first, 0);
    let (window, session) = open_with(document, selection, cx);
    let width = window
        .update(cx, |_, window, _| window.viewport_size().width)
        .unwrap();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    // The left cell has two paragraphs; the right cell only one. A click
    // level with the left tail still belongs to the right cell's blank area.
    visual.simulate_click(Point::new(width * 0.75, px(104.0)), Modifiers::default());
    cx.background_executor.run_until_parked();
    assert_eq!(inline_focus(&session).0, fixture.b1);
}

fn cell_text(document: &XiaomuDocument, cell: NodeId) -> String {
    document
        .node(cell)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .iter()
        .filter_map(|id| document.node(*id).unwrap().content().as_inline())
        .flat_map(|inline| inline.runs())
        .map(|run| run.text().as_str())
        .collect()
}

#[gpui::test]
fn rectangular_keyboard_entry_typing_and_focus_continue_after_replacement(cx: &mut TestAppContext) {
    let fixture = fixture();
    let left = fixture.document.parent_of(fixture.a1).unwrap();
    let right = fixture.document.parent_of(fixture.b1).unwrap();
    let before = fixture.document.clone();
    let selection = caret_at(&fixture.document, fixture.a1, 1);
    let (window, session) = open_with(fixture.document, selection, cx);
    cx.simulate_keystrokes(window.into(), "ctrl-shift-space shift-right");
    let range = session.borrow().selection();
    assert_eq!(range.active_cell_range().unwrap().anchor(), left);
    assert_eq!(range.active_cell_range().unwrap().focus(), right);
    assert_eq!(session.borrow().history_depths(), (0, 0));
    cx.simulate_input(window.into(), "XYZ");
    assert_eq!(cell_text(session.borrow().document(), left), "XYZ");
    assert_eq!(cell_text(session.borrow().document(), right), "");
    // Replacement is isolated; subsequent typing is a separate coalesced unit.
    cx.simulate_keystrokes(window.into(), "ctrl-z ctrl-z");
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().selection(), range);
    cx.simulate_keystrokes(window.into(), "ctrl-y ctrl-y");
    cx.simulate_input(window.into(), "Q");
    assert_eq!(cell_text(session.borrow().document(), left), "XYZQ");
}

#[gpui::test]
fn rectangular_cut_paste_and_escape_use_the_native_action_route(cx: &mut TestAppContext) {
    let fixture = fixture();
    let before = fixture.document.clone();
    let left = fixture.document.parent_of(fixture.a1).unwrap();
    let right = fixture.document.parent_of(fixture.b1).unwrap();
    let selection = caret_at(&fixture.document, fixture.a1, 0);
    let (window, session) = open_with(fixture.document, selection, cx);
    cx.simulate_keystrokes(window.into(), "ctrl-shift-space shift-right ctrl-x");
    assert_eq!(cell_text(session.borrow().document(), left), "");
    assert_eq!(cell_text(session.borrow().document(), right), "");
    assert_eq!(session.borrow().history_depths(), (1, 0));
    cx.simulate_keystrokes(window.into(), "ctrl-v");
    assert_eq!(cell_text(session.borrow().document(), left), "a1");
    assert_eq!(cell_text(session.borrow().document(), right), "b1");
    cx.simulate_keystrokes(window.into(), "ctrl-z ctrl-z escape");
    assert_eq!(session.borrow().document().store(), before.store());
    assert!(session.borrow().selection().active_cell_range().is_none());
    assert_eq!(inline_focus(&session).0, fixture.b1);
}

#[gpui::test]
fn cell_handle_drag_creates_a_rectangle_without_text_selection(cx: &mut TestAppContext) {
    let fixture = fixture();
    let left = fixture.document.parent_of(fixture.a1).unwrap();
    let right = fixture.document.parent_of(fixture.b1).unwrap();
    let selection = caret_at(&fixture.document, fixture.first, 0);
    let (window, session) = open_with(fixture.document, selection, cx);
    let width = window
        .update(cx, |_, window, _| window.viewport_size().width)
        .unwrap();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    visual.simulate_mouse_down(
        Point::new(px(20.0), px(59.0)),
        gpui::MouseButton::Left,
        Modifiers::default(),
    );
    visual.simulate_event(gpui::MouseMoveEvent {
        position: Point::new(width * 0.75, px(75.0)),
        pressed_button: Some(gpui::MouseButton::Left),
        modifiers: Modifiers::default(),
    });
    visual.simulate_mouse_up(
        Point::new(width * 0.75, px(75.0)),
        gpui::MouseButton::Left,
        Modifiers::default(),
    );
    let range = session.borrow().selection().active_cell_range().unwrap();
    assert_eq!((range.anchor(), range.focus()), (left, right));
    assert!(session.borrow().text_selection().is_none());
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
    cx.simulate_keystrokes(window_a.into(), "ctrl-shift-space shift-right");
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
