//! P5.2 cell editing: Tab/Shift+Tab navigation in reading order, the
//! last-cell row append with its exact undo/redo identities, cell-internal
//! Enter/Backspace semantics, and the typing/IME undo matrix inside a cell.

use xiaomu_core::document::{
    InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn text_of(session: &DocumentSession, node: NodeId) -> String {
    session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn children_of(document: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    document
        .node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

fn focused(session: &DocumentSession) -> (NodeId, usize) {
    match session.selection().focus() {
        DocumentPosition::Inline(point) => (point.node_id(), point.text_offset().as_usize()),
        other => panic!("expected inline focus, got {other:?}"),
    }
}

fn offset_of(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextOffset {
    document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap()
}

fn session_at(document: &XiaomuDocument, node: NodeId, raw: usize) -> DocumentSession {
    let selection = DocumentSelection::collapsed(DocumentPosition::Inline(InlinePoint::new(
        node,
        offset_of(document, node, raw),
        0,
        CursorAffinity::Before,
    )));
    DocumentSession::new(document.clone(), selection).unwrap()
}

fn cell_paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap()
}

fn cell_of(builder: &mut NodeStoreBuilder, block: NodeId) -> NodeId {
    builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap()
}

fn row_of(builder: &mut NodeStoreBuilder, cells: [NodeId; 2]) -> NodeId {
    builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells),
        )
        .unwrap()
}

/// `Document > [p("intro"), table > [row1 > [a1, b1], row2 > [a2, b2]]]`
struct TwoByTwo {
    document: XiaomuDocument,
    intro: NodeId,
    table: NodeId,
    a1: NodeId,
    b1: NodeId,
    a2: NodeId,
    b2: NodeId,
}

fn two_by_two() -> TwoByTwo {
    let mut builder = NodeStoreBuilder::new();
    let intro = cell_paragraph(&mut builder, "intro");
    let a1 = cell_paragraph(&mut builder, "a1");
    let b1 = cell_paragraph(&mut builder, "b1");
    let a2 = cell_paragraph(&mut builder, "a2");
    let b2 = cell_paragraph(&mut builder, "b2");
    let cell_a1 = cell_of(&mut builder, a1);
    let cell_b1 = cell_of(&mut builder, b1);
    let cell_a2 = cell_of(&mut builder, a2);
    let cell_b2 = cell_of(&mut builder, b2);
    let row1 = row_of(&mut builder, [cell_a1, cell_b1]);
    let row2 = row_of(&mut builder, [cell_a2, cell_b2]);
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row1, row2]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    TwoByTwo {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        table,
        a1,
        b1,
        a2,
        b2,
    }
}

#[test]
fn tab_walks_cells_in_reading_order_without_touching_history() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 2);

    assert_eq!(
        session.apply_intent(&EditIntent::MoveToNextCell).unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert_eq!(focused(&session), (fixture.b1, 0));
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(
        session.document().revision().as_u64(),
        fixture.document.revision().as_u64(),
        "navigation must not edit the document"
    );

    // From the last cell of row one, Tab enters row two's first cell.
    session.apply_intent(&EditIntent::MoveToNextCell).unwrap();
    assert_eq!(focused(&session), (fixture.a2, 0));
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn tab_on_the_last_cell_appends_one_row_and_enters_its_first_cell() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.b2, 2);

    assert_eq!(
        session.apply_intent(&EditIntent::MoveToNextCell).unwrap(),
        SessionOutcome::DocumentChanged
    );

    let rows = children_of(session.document(), fixture.table);
    assert_eq!(rows.len(), 3, "one trailing row appended");
    let cells = children_of(session.document(), rows[2]);
    assert_eq!(cells.len(), 2, "the new row mirrors the column count");
    let (node, offset) = focused(&session);
    assert_eq!(offset, 0, "the caret enters the new row's first cell");
    assert_eq!(
        session.document().parent_of(node).unwrap(),
        cells[0],
        "the caret target lives in the new row"
    );
    assert_eq!(text_of(&session, node), "");
    assert_eq!(session.history_depths(), (1, 0));

    // Undo restores the exact pre-Tab shape and selection.
    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(children_of(session.document(), fixture.table).len(), 2);
    assert_eq!(focused(&session), (fixture.b2, 2));

    // Redo reproduces the appended identities, not fresh ones.
    assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(children_of(session.document(), fixture.table).len(), 3);
    let (node_after_redo, offset_after_redo) = focused(&session);
    assert_eq!(node_after_redo, node, "redo restores the appended row ids");
    assert_eq!(offset_after_redo, 0);
}

#[test]
fn shift_tab_walks_back_and_is_a_noop_from_the_first_cell() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a2, 1);

    session
        .apply_intent(&EditIntent::MoveToPreviousCell)
        .unwrap();
    assert_eq!(focused(&session), (fixture.b1, 0));

    session
        .apply_intent(&EditIntent::MoveToPreviousCell)
        .unwrap();
    assert_eq!(focused(&session), (fixture.a1, 0));

    assert_eq!(
        session
            .apply_intent(&EditIntent::MoveToPreviousCell)
            .unwrap(),
        SessionOutcome::NoChange,
        "Shift+Tab from the very first cell does nothing"
    );
    assert_eq!(focused(&session), (fixture.a1, 0));
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn enter_splits_within_a_cell_and_backspace_joins_back_inside_it() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a2, 1);

    assert_eq!(
        session.apply_intent(&EditIntent::SplitBlock).unwrap(),
        SessionOutcome::DocumentChanged
    );
    let cell = session.document().parent_of(fixture.a2).unwrap();
    let blocks = children_of(session.document(), cell);
    assert_eq!(blocks.len(), 2, "the split stays inside the cell");
    assert_eq!(blocks[0], fixture.a2);
    let (tail, offset) = focused(&session);
    assert_eq!(tail, blocks[1]);
    assert_eq!(offset, 0);
    assert_eq!(text_of(&session, fixture.a2), "a");
    assert_eq!(text_of(&session, tail), "2");

    // Backspace at the tail start joins within the cell, never across cells.
    assert_eq!(
        session.apply_intent(&EditIntent::Backspace).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(children_of(session.document(), cell).len(), 1);
    assert_eq!(text_of(&session, fixture.a2), "a2");
    assert_eq!(focused(&session), (fixture.a2, 1), "caret at the join seam");
    assert_eq!(session.history_depths(), (2, 0));

    // The exact undo matrix: one undo per committed edit, in order.
    session.undo().unwrap();
    assert_eq!(children_of(session.document(), cell).len(), 2);
    session.undo().unwrap();
    assert_eq!(children_of(session.document(), cell).len(), 1);
    assert_eq!(text_of(&session, fixture.a2), "a2");
}

#[test]
fn backspace_at_a_cell_start_is_a_noop() {
    let fixture = two_by_two();

    // First cell of the table.
    let mut session = session_at(&fixture.document, fixture.a1, 0);
    assert_eq!(
        session.apply_intent(&EditIntent::Backspace).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(focused(&session), (fixture.a1, 0));
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(
        session.document().revision().as_u64(),
        fixture.document.revision().as_u64()
    );

    // First cell of a later row: Backspace must not merge across rows.
    let mut session = session_at(&fixture.document, fixture.a2, 0);
    assert_eq!(
        session.apply_intent(&EditIntent::Backspace).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(children_of(session.document(), fixture.table).len(), 2);
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn cell_typing_coalesces_into_one_undo_unit() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 2);

    session
        .apply_intent(&EditIntent::InsertText {
            text: "中".to_owned(),
        })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText {
            text: "文".to_owned(),
        })
        .unwrap();
    assert_eq!(text_of(&session, fixture.a1), "a1中文");
    assert_eq!(session.history_depths(), (1, 0));

    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(text_of(&session, fixture.a1), "a1");
    assert_eq!(focused(&session), (fixture.a1, 2));

    assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(text_of(&session, fixture.a1), "a1中文");
}

#[test]
fn ime_commit_inside_a_cell_is_isolated_and_reuses_stored_marks() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 2);

    // A collapsed mark toggle becomes the stored mark that typing and the
    // composition commit both consume, exactly like outside tables.
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText {
            text: "中".to_owned(),
        })
        .unwrap();
    assert_eq!(text_of(&session, fixture.a1), "a1中");
    let runs = session
        .document()
        .node(fixture.a1)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .to_vec();
    assert_eq!(runs.len(), 2);
    assert!(runs[1].marks().contains(MarkKind::Bold));

    let caret = session.text_selection().unwrap().focus().offset();
    let range = TextRange::new(caret, caret).unwrap();
    assert_eq!(
        session
            .apply_intent(&EditIntent::CommitComposition {
                range,
                text: "文".to_owned(),
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(text_of(&session, fixture.a1), "a1中文");
    assert_eq!(session.history_depths(), (2, 0));

    // The IME commit owns one isolated entry: one undo removes exactly the
    // composed text, not the preceding typing.
    session.undo().unwrap();
    assert_eq!(text_of(&session, fixture.a1), "a1中");
}

#[test]
fn tab_outside_a_table_is_a_noop() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.intro, 5);

    assert_eq!(
        session.apply_intent(&EditIntent::MoveToNextCell).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(
        session
            .apply_intent(&EditIntent::MoveToPreviousCell)
            .unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(focused(&session), (fixture.intro, 5));
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(
        session.document().revision().as_u64(),
        fixture.document.revision().as_u64()
    );
}

#[test]
fn tab_from_an_atomic_block_inside_a_cell_moves_to_the_next_cell() {
    let mut builder = NodeStoreBuilder::new();
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let text = cell_paragraph(&mut builder, "t");
    let cell_rule = cell_of(&mut builder, rule);
    let cell_text = cell_of(&mut builder, text);
    let row = row_of(&mut builder, [cell_rule, cell_text]);
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let selection = DocumentSelection::collapsed(DocumentPosition::Atomic(rule));
    let mut session = DocumentSession::new(document, selection).unwrap();

    assert_eq!(
        session.apply_intent(&EditIntent::MoveToNextCell).unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert_eq!(focused(&session), (text, 0));
    assert_eq!(session.history_depths(), (0, 0));
}
