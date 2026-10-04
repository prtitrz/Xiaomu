//! Permanent regressions from the P5 correctness review.
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

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
fn cell_of(builder: &mut NodeStoreBuilder, blocks: &[NodeId]) -> NodeId {
    builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children(blocks.to_vec()),
        )
        .unwrap()
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
        .map(|r| r.text().as_str())
        .collect()
}
fn session_at(document: &XiaomuDocument, node: NodeId, raw: usize) -> DocumentSession {
    let offset = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap();
    DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(xiaomu_runtime::session::DocumentPosition::Inline(
            InlinePoint::new(node, offset, 0, CursorAffinity::Before),
        )),
    )
    .unwrap()
}
struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    table_a: NodeId,
    cells_a: Vec<NodeId>,
    texts_a: Vec<NodeId>,
}
fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let mut rows = vec![];
    let mut cells_a = vec![];
    let mut texts_a = vec![];
    for r in [1, 2] {
        let mut cells = vec![];
        for c in ["a", "b"] {
            let text = paragraph(&mut builder, &format!("{c}{r}"));
            let cell = cell_of(&mut builder, &[text]);
            texts_a.push(text);
            cells_a.push(cell);
            cells.push(cell);
        }
        rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(cells),
                )
                .unwrap(),
        );
    }
    let table_a = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table_a]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        table_a,
        cells_a,
        texts_a,
    }
}

#[test]
fn review_column_insert_maps_gaps_in_every_row() {
    use xiaomu_core::mapping::{MapBias, MappedPosition};
    use xiaomu_core::selection::NodeGap;
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let fixture = fixture();
    let rows = children_of(&fixture.document, fixture.table_a);
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableColumn {
            table: fixture.table_a,
            index: 0,
        })
        .apply_with_changes(&fixture.document)
        .unwrap();
    for row in rows {
        assert_eq!(
            applied
                .changes()
                .map_node_gap(NodeGap::new(row, 1), MapBias::Start),
            MappedPosition::Mapped(NodeGap::new(row, 2)),
            "each row gained a cell before the old gap"
        );
    }
}

#[test]
fn review_cell_range_cut_deletes_the_copied_content() {
    let fixture = fixture();
    let mut session = session_at(&fixture.document, fixture.intro, 0);
    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[3])
        .unwrap();
    // GPUI cut does exactly these two calls (plus clipboard write).
    let copied = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(copied.plain_text(), "a1\tb1\na2\tb2");
    session.apply_intent(&EditIntent::Delete).unwrap();
    let remaining: Vec<_> = fixture
        .cells_a
        .iter()
        .map(|cell| {
            let blocks = children_of(session.document(), *cell);
            assert_eq!(blocks.len(), 1);
            text_of(&session, blocks[0])
        })
        .collect();
    assert_eq!(
        remaining,
        ["", "", "", ""],
        "cut must clear the selected cells, not just one scalar"
    );
    let cleared = session.document().clone();
    assert_eq!(session.history_depths(), (1, 0));
    assert!(session.selection().active_cell_range().is_some());
    session.undo().unwrap();
    assert_eq!(session.document().store(), fixture.document.store());
    assert_eq!(text_of(&session, fixture.texts_a[0]), "a1");
    session.redo().unwrap();
    assert_eq!(session.document().store(), cleared.store());
}

#[test]
fn review_cell_copy_supports_a_legal_quote() {
    let mut builder = NodeStoreBuilder::new();
    let text = paragraph(&mut builder, "inside quote");
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([text]),
        )
        .unwrap();
    let cell = cell_of(&mut builder, &[quote]);
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell]),
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
    let mut session = session_at(&document, text, 0);
    session.set_cell_range_selection(cell, cell).unwrap();
    assert!(
        session.clipboard_slice().is_ok(),
        "canonical-valid cell content must be copyable"
    );
}

#[test]
fn review_table_copy_preserves_unknown_attrs() {
    use xiaomu_core::document::AttrValue;
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let fixture = fixture();
    let attrs = NodeAttrs::new(
        [(
            "extension-tag".to_string(),
            AttrValue::String("retain me".into()),
        )]
        .into(),
    )
    .unwrap();
    let document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: fixture.table_a,
            attrs: attrs.clone(),
        })
        .apply(&fixture.document)
        .unwrap();
    let mut session = session_at(&document, fixture.intro, 0);
    session
        .set_cell_range_selection(fixture.cells_a[0], fixture.cells_a[3])
        .unwrap();
    let copied = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(copied.roots()[0].attrs(), &attrs);
}

#[test]
fn rectangular_commands_never_silently_edit_only_the_first_cell() {
    use xiaomu_core::document::{Mark, MarkKind};
    use xiaomu_runtime::session::{SessionError, SessionOutcome};
    let fixture = fixture();
    let mut session = session_at(&fixture.document, fixture.intro, 0);
    // Backwards endpoints exercise normalization and anchor-based typing.
    session
        .set_cell_range_selection(fixture.cells_a[3], fixture.cells_a[0])
        .unwrap();
    let range = session.selection();
    assert!(!range.is_collapsed());
    assert!(session.text_selection().is_none());
    assert!(matches!(
        session.apply_intent(&EditIntent::SplitBlock),
        Err(SessionError::SelectionInvalid)
    ));
    assert_eq!(session.document().store(), fixture.document.store());
    assert_eq!(session.selection(), range);
    assert_eq!(session.history_depths(), (0, 0));
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    for text in &fixture.texts_a {
        let inline = session
            .document()
            .node(*text)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        assert!(
            inline
                .runs()
                .iter()
                .all(|run| run.marks().contains(MarkKind::Bold))
        );
    }
    assert_eq!(
        session.document().node(fixture.intro),
        fixture.document.node(fixture.intro)
    );
    assert_eq!(session.selection(), range);
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), fixture.document.store());
    assert_eq!(session.selection(), range);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    let cleared = session.document().clone();
    assert_eq!(
        session.apply_intent(&EditIntent::Delete).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(session.document().revision(), cleared.revision());
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    session
        .apply_intent(&EditIntent::InsertText {
            text: "中🙂".into(),
        })
        .unwrap();
    for (index, cell) in fixture.cells_a.iter().enumerate() {
        let child = children_of(session.document(), *cell)[0];
        assert_eq!(
            text_of(&session, child),
            if index == 3 { "中🙂" } else { "" }
        );
    }
    let text_selection = session.text_selection().unwrap();
    assert_eq!(text_selection.focus().offset().as_usize(), "中🙂".len());
    session.undo().unwrap();
    assert_eq!(session.document().store(), fixture.document.store());
    assert_eq!(session.selection(), range);
}

#[test]
fn column_gap_mapping_round_trips_for_both_biases_and_every_boundary() {
    use xiaomu_core::mapping::{MapBias, MappedPosition};
    use xiaomu_core::selection::NodeGap;
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let fixture = fixture();
    let rows = children_of(&fixture.document, fixture.table_a);
    for insertion in 0..=2 {
        let applied = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertTableColumn {
                table: fixture.table_a,
                index: insertion,
            })
            .apply_with_changes(&fixture.document)
            .unwrap();
        let mut undo = Transaction::new(TransactionOrigin::UserInput);
        for step in applied.inverse().steps() {
            undo.push_step(step.clone());
        }
        let undone = undo.apply_with_changes(applied.document()).unwrap();
        assert_eq!(undone.document().store(), fixture.document.store());
        for row in &rows {
            for index in 0..=2 {
                for bias in [MapBias::Start, MapBias::End] {
                    let gap = NodeGap::new(*row, index);
                    let MappedPosition::Mapped(mapped) = applied.changes().map_node_gap(gap, bias)
                    else {
                        panic!("insertion cannot delete an existing gap");
                    };
                    assert_eq!(
                        undone.changes().map_node_gap(mapped, bias),
                        MappedPosition::Mapped(gap)
                    );
                }
            }
        }
    }
}
