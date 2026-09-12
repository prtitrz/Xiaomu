//! P5.3 row/column operations: indexed row/column insertion, row/column
//! deletion with caret convergence to structural seams, exact undo/redo
//! identities, fail-closed last row/column, and atom payload coexistence.

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap};
use xiaomu_core::text::{TextBuffer, TextOffset};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionError, SessionOutcome,
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

fn cell_text(session: &DocumentSession, cell: NodeId) -> String {
    text_of(session, children_of(session.document(), cell)[0])
}

fn focused(session: &DocumentSession) -> (NodeId, usize) {
    match session.selection().focus() {
        DocumentPosition::Inline(point) => (point.node_id(), point.text_offset().as_usize()),
        other => panic!("expected inline focus, got {other:?}"),
    }
}

fn focused_gap(session: &DocumentSession) -> (NodeId, usize) {
    match session.selection().focus() {
        DocumentPosition::Gap(gap) => (gap.parent(), gap.index()),
        other => panic!("expected gap focus, got {other:?}"),
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

/// `Document > [table > [row1 > [a1, b1], row2 > [a2, b2]]]`
struct TwoByTwo {
    document: XiaomuDocument,
    table: NodeId,
    row1: NodeId,
    row2: NodeId,
    a1: NodeId,
    b1: NodeId,
    a2: NodeId,
    b2: NodeId,
}

fn two_by_two() -> TwoByTwo {
    let mut builder = NodeStoreBuilder::new();
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
            NodeContent::children([table]),
        )
        .unwrap();
    TwoByTwo {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        row1,
        row2,
        a1,
        b1,
        a2,
        b2,
    }
}

/// Inserts one mention atom into `parent` at `byte` and returns its id.
fn insert_atom(
    document: &mut XiaomuDocument,
    parent: NodeId,
    byte: usize,
    fallback: &str,
) -> NodeId {
    let text = document
        .node(parent)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect::<String>();
    let offset = TextBuffer::from_string(text).offset_at(byte).unwrap();
    *document = Transaction::new(TransactionOrigin::Extension("p5-ops-test".into()))
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(parent, offset, 0, CursorAffinity::Before),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(fallback).unwrap(),
        })
        .apply(document)
        .unwrap();
    document
        .node(parent)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| placement.atom())
        .next()
        .unwrap()
}

#[test]
fn insert_row_at_index_preserves_existing_rows_and_the_caret() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.b1, 2);

    assert_eq!(
        session
            .apply_intent(&EditIntent::InsertTableRow {
                table: fixture.table,
                index: 1,
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );

    let rows = children_of(session.document(), fixture.table);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], fixture.row1, "rows before the index stay put");
    assert_eq!(rows[2], fixture.row2, "rows after the index stay put");
    for cell in children_of(session.document(), rows[1]) {
        let blocks = children_of(session.document(), cell);
        assert_eq!(blocks.len(), 1, "one empty paragraph per new cell");
        assert_eq!(text_of(&session, blocks[0]), "");
    }
    assert_eq!(focused(&session), (fixture.b1, 2), "caret maps through");
    assert_eq!(session.history_depths(), (1, 0));

    // Exact undo/redo identities.
    session.undo().unwrap();
    assert_eq!(children_of(session.document(), fixture.table).len(), 2);
    assert_eq!(focused(&session), (fixture.b1, 2));
    session.redo().unwrap();
    let rows = children_of(session.document(), fixture.table);
    assert_eq!(rows.len(), 3);
    assert_eq!(text_of(&session, fixture.b1), "b1");
}

#[test]
fn insert_row_at_the_end_appends_like_the_tab_seam() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 0);

    session
        .apply_intent(&EditIntent::InsertTableRow {
            table: fixture.table,
            index: 2,
        })
        .unwrap();
    let rows = children_of(session.document(), fixture.table);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], fixture.row1);
    assert_eq!(rows[1], fixture.row2);
    session.undo().unwrap();
    assert_eq!(children_of(session.document(), fixture.table).len(), 2);
}

#[test]
fn insert_column_at_index_adds_one_cell_per_row() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a2, 1);

    assert_eq!(
        session
            .apply_intent(&EditIntent::InsertTableColumn {
                table: fixture.table,
                index: 1,
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );

    // Existing text sits in the outer columns, untouched; the new middle
    // column is empty in every row.
    assert_eq!(
        cell_text(&session, children_of(session.document(), fixture.row1)[0]),
        "a1"
    );
    assert_eq!(
        cell_text(&session, children_of(session.document(), fixture.row1)[2]),
        "b1"
    );
    for row in [fixture.row1, fixture.row2] {
        let cells = children_of(session.document(), row);
        assert_eq!(cells.len(), 3, "one cell added per row");
        assert_eq!(cell_text(&session, cells[1]), "");
    }
    assert_eq!(focused(&session), (fixture.a2, 1), "caret maps through");
    assert_eq!(session.history_depths(), (1, 0));

    session.undo().unwrap();
    assert_eq!(children_of(session.document(), fixture.row1).len(), 2);
    session.redo().unwrap();
    assert_eq!(children_of(session.document(), fixture.row1).len(), 3);
    assert_eq!(
        children_of(session.document(), fixture.row2).len(),
        3,
        "redo restores every row's cell identity"
    );
}

#[test]
fn insert_operations_with_out_of_range_index_fail_closed() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 0);

    match session.apply_intent(&EditIntent::InsertTableRow {
        table: fixture.table,
        index: 3,
    }) {
        Err(SessionError::SelectionInvalid) => {}
        other => panic!("expected SelectionInvalid, got {other:?}"),
    }
    match session.apply_intent(&EditIntent::InsertTableColumn {
        table: fixture.table,
        index: 3,
    }) {
        Err(SessionError::SelectionInvalid) => {}
        other => panic!("expected SelectionInvalid, got {other:?}"),
    }
    match session.apply_intent(&EditIntent::InsertTableRow {
        table: fixture.a1,
        index: 0,
    }) {
        Err(SessionError::SelectionInvalid) => {}
        other => panic!("non-table target must fail closed, got {other:?}"),
    }
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(
        session.document().revision().as_u64(),
        fixture.document.revision().as_u64()
    );
}

#[test]
fn delete_row_converges_an_inside_caret_to_the_seam() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a2, 2);

    assert_eq!(
        session
            .apply_intent(&EditIntent::DeleteTableRow {
                table: fixture.table,
                index: 1,
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );

    let rows = children_of(session.document(), fixture.table);
    assert_eq!(rows, vec![fixture.row1]);
    assert_eq!(
        focused_gap(&session),
        (fixture.table, 1),
        "the caret converges to the seam where the row was"
    );
    assert_eq!(session.history_depths(), (1, 0));

    // Undo restores the row identities and the caret inside it.
    session.undo().unwrap();
    assert_eq!(children_of(session.document(), fixture.table).len(), 2);
    assert_eq!(focused(&session), (fixture.a2, 2));

    session.redo().unwrap();
    assert_eq!(children_of(session.document(), fixture.table).len(), 1);
    assert_eq!(focused_gap(&session), (fixture.table, 1));
}

#[test]
fn delete_row_maps_an_outside_caret_through_unchanged() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.b1, 2);

    session
        .apply_intent(&EditIntent::DeleteTableRow {
            table: fixture.table,
            index: 1,
        })
        .unwrap();
    assert_eq!(focused(&session), (fixture.b1, 2));
    assert_eq!(text_of(&session, fixture.b1), "b1");
    session.undo().unwrap();
    assert_eq!(focused(&session), (fixture.b1, 2));
}

#[test]
fn delete_column_converges_a_deleted_cell_caret_and_keeps_other_cells() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.b2, 2);

    assert_eq!(
        session
            .apply_intent(&EditIntent::DeleteTableColumn {
                table: fixture.table,
                index: 1,
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );

    assert_eq!(children_of(session.document(), fixture.row1).len(), 1);
    assert_eq!(children_of(session.document(), fixture.row2).len(), 1);
    assert_eq!(
        focused_gap(&session),
        (fixture.row2, 1),
        "the caret converges to the seam in its own row"
    );
    assert_eq!(session.history_depths(), (1, 0));

    session.undo().unwrap();
    assert_eq!(children_of(session.document(), fixture.row1).len(), 2);
    assert_eq!(focused(&session), (fixture.b2, 2));
    session.redo().unwrap();
    assert_eq!(children_of(session.document(), fixture.row2).len(), 1);
    assert_eq!(focused_gap(&session), (fixture.row2, 1));
}

#[test]
fn delete_column_keeps_a_caret_in_a_surviving_cell() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 0);

    session
        .apply_intent(&EditIntent::DeleteTableColumn {
            table: fixture.table,
            index: 1,
        })
        .unwrap();
    assert_eq!(focused(&session), (fixture.a1, 0));
    assert_eq!(text_of(&session, fixture.a1), "a1");
    session.undo().unwrap();
    assert_eq!(children_of(session.document(), fixture.row2).len(), 2);
    assert_eq!(focused(&session), (fixture.a1, 0));
}

#[test]
fn deleting_the_last_row_or_column_fails_closed() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 0);

    session
        .apply_intent(&EditIntent::DeleteTableRow {
            table: fixture.table,
            index: 1,
        })
        .unwrap();
    match session.apply_intent(&EditIntent::DeleteTableRow {
        table: fixture.table,
        index: 0,
    }) {
        Err(SessionError::Core(xiaomu_core::Error::InvalidTableStructure)) => {}
        other => panic!("expected InvalidTableStructure, got {other:?}"),
    }

    // Same for the last column.
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 0);
    session
        .apply_intent(&EditIntent::DeleteTableColumn {
            table: fixture.table,
            index: 1,
        })
        .unwrap();
    match session.apply_intent(&EditIntent::DeleteTableColumn {
        table: fixture.table,
        index: 0,
    }) {
        Err(SessionError::Core(xiaomu_core::Error::InvalidTableStructure)) => {}
        other => panic!("expected InvalidTableStructure, got {other:?}"),
    }
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn deleted_row_payloads_restore_exactly() {
    let mut fixture = two_by_two();
    let atom = insert_atom(&mut fixture.document, fixture.a2, 0, "@甲");
    let mut session = session_at(&fixture.document, fixture.a2, 0);

    session
        .apply_intent(&EditIntent::DeleteTableRow {
            table: fixture.table,
            index: 1,
        })
        .unwrap();
    assert_eq!(children_of(session.document(), fixture.table).len(), 1);

    session.undo().unwrap();
    let document = session.document();
    let inline = document
        .node(fixture.a2)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    assert_eq!(
        inline
            .atoms()
            .iter()
            .map(|placement| placement.atom())
            .collect::<Vec<_>>(),
        vec![atom],
        "the atom identity survives the row round-trip"
    );
    assert_eq!(
        inline
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect::<String>(),
        "a2",
        "the surviving run text is untouched"
    );
}

#[test]
fn operations_coexist_with_typing_history() {
    let fixture = two_by_two();
    let mut session = session_at(&fixture.document, fixture.a1, 0);

    session
        .apply_intent(&EditIntent::InsertTableColumn {
            table: fixture.table,
            index: 0,
        })
        .unwrap();
    let new_cell = children_of(session.document(), fixture.row1)[0];
    let new_paragraph = children_of(session.document(), new_cell)[0];
    session
        .apply_intent(&EditIntent::InsertText {
            text: "中".to_owned(),
        })
        .unwrap();
    // The caret stayed in a1, so the text lands there while the new cell
    // stays empty.
    assert_eq!(text_of(&session, fixture.a1), "中a1");
    assert_eq!(text_of(&session, new_paragraph), "");
    assert_eq!(session.history_depths(), (2, 0));

    session.undo().unwrap();
    assert_eq!(text_of(&session, fixture.a1), "a1");
    session.undo().unwrap();
    assert_eq!(children_of(session.document(), fixture.row1).len(), 2);
    assert_eq!(focused(&session), (fixture.a1, 0));
}

#[test]
fn gap_focus_inside_a_deleted_cell_also_converges() {
    let fixture = two_by_two();
    // Split a2 so its cell holds two paragraphs, then place the caret on
    // the seam between them.
    let mut session = session_at(&fixture.document, fixture.a2, 1);
    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let cell = session.document().parent_of(fixture.a2).unwrap();
    assert_eq!(children_of(session.document(), cell).len(), 2);
    let tail = children_of(session.document(), cell)[1];

    let document = session.document().clone();
    let selection = DocumentSelection::collapsed(DocumentPosition::Gap(NodeGap::new(cell, 1)));
    let mut session = DocumentSession::new(document, selection).unwrap();

    session
        .apply_intent(&EditIntent::DeleteTableColumn {
            table: fixture.table,
            index: 0,
        })
        .unwrap();
    assert_eq!(
        focused_gap(&session),
        (fixture.row2, 0),
        "a gap inside the deleted cell converges to the row seam"
    );
    session.undo().unwrap();
    assert!(session.document().node(tail).is_some());
}
