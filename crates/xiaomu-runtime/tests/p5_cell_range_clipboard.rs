//! P5.5 cell-range selection and clipboard: rectangular selection with
//! validation and mapping, wire v5 table payloads with v4 fail-soft, the TSV
//! plain-text fallback, and the paste matrix (range replacement, single-cell
//! entry, sibling table, fail-closed placements).

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};
use xiaomu_runtime::session::{
    CellRange, DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionError,
    SessionOutcome,
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

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
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

fn cell_of(builder: &mut NodeStoreBuilder, blocks: &[NodeId]) -> NodeId {
    builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children(blocks.to_vec()),
        )
        .unwrap()
}

/// `Document > [intro, tableA(2×2), tableB(2×2), outro]`.
struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    table_a: NodeId,
    cells_a: Vec<NodeId>,
    texts_a: Vec<NodeId>,
    table_b: NodeId,
    cells_b: Vec<NodeId>,
    texts_b: Vec<NodeId>,
}

/// Returns `(table, cells row-major, cell paragraphs row-major)`.
fn two_by_two_table(
    builder: &mut NodeStoreBuilder,
    prefix: &str,
) -> (NodeId, Vec<NodeId>, Vec<NodeId>) {
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut texts = Vec::new();
    for label in ["1", "2"] {
        let mut row_cells = Vec::new();
        for column in ["a", "b"] {
            let text = paragraph(builder, &format!("{prefix}{column}{label}"));
            let cell = cell_of(builder, &[text]);
            row_cells.push(cell);
            cells.push(cell);
            texts.push(text);
        }
        rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(row_cells),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    (table, cells, texts)
}

fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let (table_a, cells_a, texts_a) = two_by_two_table(&mut builder, "");
    let (table_b, cells_b, texts_b) = two_by_two_table(&mut builder, "s");
    let outro = paragraph(&mut builder, "outro");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table_a, table_b, outro]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        table_a,
        cells_a,
        texts_a,
        table_b,
        cells_b,
        texts_b,
    }
}

#[test]
fn cell_range_validates_and_chooses_one_table() {
    let fixture = fixture();
    let mut session = session_at(&fixture.document, fixture.intro, 0);

    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[3])
        .unwrap();
    let range = session.selection().active_cell_range().unwrap();
    assert_eq!(range.anchor(), fixture.cells_a[0]);
    assert_eq!(range.focus(), fixture.cells_a[3]);

    // Cross-table rectangles fail closed.
    match session.set_cell_range_selection(fixture.cells_a[0], fixture.cells_b[3]) {
        Err(SessionError::SelectionInvalid) => {}
        other => panic!("expected SelectionInvalid, got {other:?}"),
    }
    // Non-cell endpoints fail closed.
    match session.set_cell_range_selection(fixture.intro, fixture.cells_a[3]) {
        Err(SessionError::SelectionInvalid) => {}
        other => panic!("expected SelectionInvalid, got {other:?}"),
    }
    assert!(matches!(
        fixture.document.node(fixture.table_b).unwrap().kind(),
        NodeKind::Table
    ));
    assert_eq!(text_of(&session, fixture.texts_b[0]), "sa1");
}

#[test]
fn a_content_intent_collapses_the_range_back_to_the_parked_caret() {
    let fixture = fixture();
    let mut session = session_at(&fixture.document, fixture.intro, 0);
    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[3])
        .unwrap();

    session
        .apply_intent(&EditIntent::InsertText {
            text: "中".to_owned(),
        })
        .unwrap();
    assert!(session.selection().active_cell_range().is_none());
    // The range converged onto the anchor cell's first block start before
    // the intent ran, so typing landed there.
    let (node, offset) = match session.selection().focus() {
        DocumentPosition::Inline(point) => (point.node_id(), point.text_offset().as_usize()),
        other => panic!("expected inline focus, got {other:?}"),
    };
    assert_eq!(node, fixture.texts_a[0]);
    assert_eq!(offset, 3, "the caret sits after the typed scalar");
    assert_eq!(text_of(&session, fixture.texts_a[0]), "中a1");
}

#[test]
fn copy_round_trips_wire_v5_with_tsv_fallback() {
    let fixture = fixture();
    let mut session = session_at(&fixture.document, fixture.intro, 0);
    session
        .set_cell_range_selection(fixture.cells_b[1], fixture.cells_b[3])
        .unwrap();

    let slice = session.clipboard_slice().unwrap().unwrap();
    // Rect rows 0..=1, columns 1..=1: s b1 / s b2.
    assert_eq!(slice.plain_text(), "sb1\nsb2");

    let metadata = encode_metadata(&slice).unwrap();
    assert!(
        metadata.contains("\"version\":5"),
        "table payloads bump to v5"
    );
    let decoded = decode_metadata(slice.plain_text(), &metadata).unwrap();
    assert_eq!(decoded.plain_text(), slice.plain_text());
    assert!(decoded.roots()[0].content().as_table().is_some());
}

#[test]
fn non_table_slices_stay_at_wire_v4() {
    let fixture = fixture();
    let anchor = DocumentPosition::Inline(InlinePoint::new(
        fixture.texts_a[0],
        offset_of(&fixture.document, fixture.texts_a[0], 0),
        0,
        CursorAffinity::Before,
    ));
    let focus = DocumentPosition::Inline(InlinePoint::new(
        fixture.texts_a[0],
        offset_of(&fixture.document, fixture.texts_a[0], 2),
        0,
        CursorAffinity::Before,
    ));
    let session = DocumentSession::new(
        fixture.document.clone(),
        DocumentSelection::new(anchor, focus),
    )
    .unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "a1");
    let metadata = encode_metadata(&slice).unwrap();
    assert!(metadata.contains("\"version\":4"));
    assert!(decode_metadata(slice.plain_text(), &metadata).is_some());
}

#[test]
fn wire_v5_table_payload_fails_soft_against_v4_guards() {
    // A v5 envelope is rejected wholesale by the version guard when its
    // computed fallback disagrees, and a table tag inside a v4 envelope
    // fails deserialization: both fall back to plain text (None here, since
    // the fake body does not match).
    let metadata = r#"{"format":"xiaomu.clipboard","version":4,"roots":[{"kind":{"type":"table"},"attrs":{},"content":{"type":"table","rows":[]}}]}"#;
    assert!(decode_metadata("anything", metadata).is_none());
}

#[test]
fn paste_replaces_a_matching_cell_range_as_one_history_entry() {
    let fixture = fixture();
    // Copy the 2×2 rectangle of table B.
    let mut source = session_at(&fixture.document, fixture.intro, 0);
    source
        .set_cell_range_selection(fixture.cells_b[0], fixture.cells_b[3])
        .unwrap();
    let slice = source.clipboard_slice().unwrap().unwrap();

    // Paste it over table A's full rectangle.
    let mut session = session_at(&fixture.document, fixture.intro, 0);
    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[3])
        .unwrap();
    assert_eq!(
        session
            .apply_intent(&EditIntent::PasteSlice {
                slice: slice.clone(),
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );

    let cell_texts = |session: &DocumentSession| -> Vec<String> {
        fixture
            .cells_a
            .iter()
            .map(|cell| {
                text_of(
                    session,
                    children_of(session.document(), *cell)
                        .first()
                        .copied()
                        .unwrap(),
                )
            })
            .collect()
    };
    // The pasted content carries table B's texts; the original block ids in
    // table A's cells were removed and replaced.
    assert_eq!(cell_texts(&session), vec!["sa1", "sb1", "sa2", "sb2"]);
    assert_eq!(session.history_depths(), (1, 0));

    session.undo().unwrap();
    assert_eq!(
        cell_texts(&session),
        vec!["a1", "b1", "a2", "b2"],
        "undo restores the originals"
    );
}

#[test]
fn paste_with_mismatched_dimensions_fails_closed() {
    let fixture = fixture();
    // Copy a 1×1 range of table B.
    let mut source = session_at(&fixture.document, fixture.intro, 0);
    source
        .set_cell_range_selection(fixture.cells_b[0], fixture.cells_b[0])
        .unwrap();
    let slice = source.clipboard_slice().unwrap().unwrap();

    let mut session = session_at(&fixture.document, fixture.intro, 0);
    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[3])
        .unwrap();
    match session.apply_intent(&EditIntent::PasteSlice { slice }) {
        Err(SessionError::ClipboardTableUnsupported) => {}
        other => panic!("expected ClipboardTableUnsupported, got {other:?}"),
    }
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(text_of(&session, fixture.texts_a[0]), "a1");
}

#[test]
fn a_single_cell_payload_enters_the_focused_cell() {
    let fixture = fixture();
    // Copy the 1×1 rectangle at table B's first cell.
    let mut source = session_at(&fixture.document, fixture.intro, 0);
    source
        .set_cell_range_selection(fixture.cells_b[0], fixture.cells_b[0])
        .unwrap();
    let slice = source.clipboard_slice().unwrap().unwrap();

    // Paste into a cell of table A with the caret in its block.
    let target_block = fixture.texts_a[1];
    let mut session = session_at(&fixture.document, target_block, 2);
    session
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();

    let target_cell = fixture.document.parent_of(target_block).unwrap();
    let blocks = children_of(session.document(), target_cell);
    assert_eq!(blocks.len(), 2, "the payload block joined the cell");
    assert_eq!(
        text_of(&session, blocks[0]),
        "b1",
        "the original block stays first"
    );
    assert_eq!(
        text_of(&session, blocks[1]),
        "sa1",
        "the payload block follows"
    );
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn a_table_payload_pastes_as_a_sibling_of_a_plain_block() {
    let fixture = fixture();
    // Copy table B's 2×2 rectangle.
    let mut source = session_at(&fixture.document, fixture.intro, 0);
    source
        .set_cell_range_selection(fixture.cells_b[0], fixture.cells_b[3])
        .unwrap();
    let slice = source.clipboard_slice().unwrap().unwrap();

    // Caret on the intro paragraph (a plain block outside any table).
    let mut session = session_at(&fixture.document, fixture.intro, 5);
    session
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();

    let root_children = children_of(session.document(), session.document().root());
    assert_eq!(
        root_children.len(),
        5,
        "intro, new table, tableA, tableB, outro"
    );
    let pasted = root_children[1];
    assert!(matches!(
        session.document().node(pasted).unwrap().kind(),
        NodeKind::Table
    ));
    let rows = children_of(session.document(), pasted);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        let cells = children_of(session.document(), *row);
        assert_eq!(cells.len(), 2);
        for cell in cells {
            let blocks = children_of(session.document(), cell);
            assert_eq!(blocks.len(), 1, "the seed paragraph was removed");
            assert!(!text_of(&session, blocks[0]).is_empty());
        }
    }
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(
        children_of(session.document(), session.document().root()).len(),
        4
    );
}

#[test]
fn structural_row_deletion_shrinks_an_active_range() {
    let fixture = fixture();
    let mut session = session_at(&fixture.document, fixture.intro, 0);
    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[3])
        .unwrap();

    // Delete the first row: the range shrinks to the surviving row's cells.
    session
        .apply_intent(&EditIntent::DeleteTableRow {
            table: fixture.table_a,
            index: 0,
        })
        .unwrap();
    let range = session
        .selection()
        .active_cell_range()
        .expect("one endpoint survives");
    // The rectangle collapses onto the single surviving cell, and the
    // parked caret converges to the seam where the deleted row was.
    assert_eq!(range.anchor(), fixture.cells_a[3]);
    assert_eq!(range.focus(), fixture.cells_a[3]);
}

#[test]
fn whole_range_deletion_converges_to_the_mapped_caret() {
    let fixture = fixture();
    let mut session = session_at(&fixture.document, fixture.intro, 0);
    // A single-row range dies with its row.
    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[1])
        .unwrap();
    session
        .apply_intent(&EditIntent::DeleteTableRow {
            table: fixture.table_a,
            index: 0,
        })
        .unwrap();
    assert!(session.selection().active_cell_range().is_none());
    // The parked caret converged to the seam where the row was.
    match session.selection().focus() {
        DocumentPosition::Gap(gap) => {
            assert_eq!(gap.parent(), fixture.table_a);
            assert_eq!(gap.index(), 0);
        }
        other => panic!("expected gap focus, got {other:?}"),
    }
    session.undo().unwrap();
    let range = session.selection().active_cell_range().unwrap();
    assert_eq!(range.anchor(), fixture.cells_a[0]);
}

#[test]
fn cell_range_copy_tsv_flattens_in_cell_blocks() {
    // Build one table whose first cell holds two paragraphs.
    let mut builder = NodeStoreBuilder::new();
    let first = paragraph(&mut builder, "甲");
    let second = paragraph(&mut builder, "乙");
    let normal = paragraph(&mut builder, "c2");
    let multi_cell = cell_of(&mut builder, &[first, second]);
    let normal_cell = cell_of(&mut builder, &[normal]);
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([multi_cell, normal_cell]),
        )
        .unwrap();
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
    let mut session = session_at(&document, first, 0);
    session
        .set_cell_range_selection(multi_cell, normal_cell)
        .unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(
        slice.plain_text(),
        "甲 乙\tc2",
        "in-cell block boundaries flatten"
    );
}

#[test]
fn cell_range_type_is_public_and_debuggable() {
    let fixture = fixture();
    let mut session = session_at(&fixture.document, fixture.intro, 0);
    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[3])
        .unwrap();
    let range: CellRange = session.selection().active_cell_range().unwrap();
    assert_eq!(range.anchor(), fixture.cells_a[0]);
}
