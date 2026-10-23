//! Regressions for the P5.6 project audit (2026-10-23).

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection, DocumentSession, EditIntent};

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

/// `Document > [intro, tableA(2×2), outro]`.
struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    table_a: NodeId,
    cells_a: Vec<NodeId>,
    texts_a: Vec<NodeId>,
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
    let outro = paragraph(&mut builder, "outro");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table_a, outro]),
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
fn insert_column_maps_second_row_gap() {
    use xiaomu_core::{
        mapping::{MapBias, MappedPosition},
        selection::NodeGap,
        transaction::{Transaction, TransactionOrigin, TransactionStep},
    };
    let f = fixture();
    let row = f.document.parent_of(f.cells_a[2]).unwrap();
    let old = NodeGap::new(row, 1);
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableColumn {
            table: f.table_a,
            index: 0,
        })
        .apply_with_changes(&f.document)
        .unwrap();
    assert_eq!(
        applied.changes().map_node_gap(old, MapBias::End),
        MappedPosition::Mapped(NodeGap::new(row, 2))
    );
}

#[test]
fn cell_range_cut_deletes_the_copied_cells() {
    let f = fixture();
    let mut session = session_at(&f.document, f.intro, 0);
    session
        .set_cell_range_selection(f.cells_a[0], f.cells_a[3])
        .unwrap();
    let copied = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(copied.plain_text(), "a1\tb1\na2\tb2");
    // Same operation sequence as DocumentView::cut.
    session.apply_intent(&EditIntent::Delete).unwrap();
    let contents: Vec<_> = f
        .cells_a
        .iter()
        .map(|cell| {
            let blocks = children_of(session.document(), *cell);
            assert_eq!(blocks.len(), 1);
            text_of(&session, blocks[0])
        })
        .collect();
    assert_eq!(contents, vec!["", "", "", ""]);
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.clipboard_slice().unwrap().unwrap(), copied);
    session.redo().unwrap();
    assert_eq!(
        session.clipboard_slice().unwrap().unwrap().plain_text(),
        "\t\n\t"
    );
}

#[test]
fn sibling_paste_preserves_cell_attrs() {
    use std::collections::BTreeMap;
    use xiaomu_core::{
        document::AttrValue,
        transaction::{Transaction, TransactionOrigin, TransactionStep},
    };
    let f = fixture();
    let attrs = NodeAttrs::new(BTreeMap::from([(
        "extension-data".into(),
        AttrValue::String("retain-me".into()),
    )]))
    .unwrap();
    let document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: f.cells_a[0],
            attrs: attrs.clone(),
        })
        .apply(&f.document)
        .unwrap();
    let mut session = session_at(&document, f.intro, 0);
    session
        .set_cell_range_selection(f.cells_a[0], f.cells_a[3])
        .unwrap();
    let copied = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(
        copied.roots()[0].content().as_table().unwrap()[0][0].attrs(),
        &attrs
    );
    session
        .set_inline_selection(
            InlinePoint::at_start_of(f.intro),
            InlinePoint::at_start_of(f.intro),
        )
        .unwrap();
    session
        .apply_intent(&EditIntent::PasteSlice { slice: copied })
        .unwrap();
    let new_table = children_of(session.document(), session.document().root())[1];
    let new_row = children_of(session.document(), new_table)[0];
    let new_cell = children_of(session.document(), new_row)[0];
    assert_eq!(session.document().node(new_cell).unwrap().attrs(), &attrs);
}

#[test]
fn table_row_rejects_non_table_without_panic() {
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let mut builder = NodeStoreBuilder::new();
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([quote]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, builder.finish()).unwrap();
    let result = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableRow {
            table: root,
            index: 0,
        })
        .apply(&doc);
    assert!(result.is_err());
}

#[test]
fn copy_cell_with_quote_succeeds() {
    let mut b = NodeStoreBuilder::new();
    let p = paragraph(&mut b, "inside");
    let q = b
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([p]),
        )
        .unwrap();
    let c = cell_of(&mut b, &[q]);
    let row = b
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([c]),
        )
        .unwrap();
    let table = b
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let mut session = session_at(&doc, p, 0);
    session.set_cell_range_selection(c, c).unwrap();
    assert!(
        session.clipboard_slice().is_ok(),
        "a valid cell containing a quote should copy"
    );
}

#[test]
fn tsv_keeps_one_row_for_hardbreak() {
    let mut b = NodeStoreBuilder::new();
    let p = paragraph(&mut b, "line1\nline2");
    let c = cell_of(&mut b, &[p]);
    let row = b
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([c]),
        )
        .unwrap();
    let table = b
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let mut session = session_at(&doc, p, 0);
    session.set_cell_range_selection(c, c).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "line1 line2");
    // Old v5 bodies had literal newlines. Verify against that old contract
    // before upgrading the decoded slice's fallback to v6 normalization.
    use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};
    let mut legacy: serde_json::Value =
        serde_json::from_str(&encode_metadata(&slice).unwrap()).unwrap();
    legacy["version"] = 5.into();
    legacy["roots"][0]["content"]["value"]
        .as_object_mut()
        .unwrap()
        .remove("row_attrs");
    assert_eq!(
        decode_metadata("line1\nline2", &legacy.to_string()).unwrap(),
        slice
    );
    assert!(decode_metadata("stale body", &legacy.to_string()).is_none());
}

#[test]
fn column_insertion_maps_every_row_gap_with_both_biases_and_exact_inverse() {
    use xiaomu_core::{
        mapping::{MapBias, MappedPosition},
        selection::NodeGap,
        transaction::{Transaction, TransactionOrigin, TransactionStep},
    };
    let f = fixture();
    for inserted in 0..=2 {
        let applied = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertTableColumn {
                table: f.table_a,
                index: inserted,
            })
            .apply_with_changes(&f.document)
            .unwrap();
        for row in children_of(&f.document, f.table_a) {
            for gap in 0..=2 {
                for bias in [MapBias::Start, MapBias::End] {
                    let shift = gap > inserted || (gap == inserted && bias == MapBias::End);
                    assert_eq!(
                        applied.changes().map_node_gap(NodeGap::new(row, gap), bias),
                        MappedPosition::Mapped(NodeGap::new(row, gap + usize::from(shift)))
                    );
                }
            }
        }
        for text in &f.texts_a {
            assert_eq!(applied.document().node(*text), f.document.node(*text));
        }
        let restored = applied.inverse().apply(applied.document()).unwrap();
        assert_eq!(restored.store(), f.document.store());
    }
}

#[test]
fn table_edits_reject_wrong_target_and_invalid_intermediate_shape_atomically() {
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let f = fixture();
    for target in [
        f.document.root(),
        f.cells_a[0],
        f.texts_a[0],
        f.document.parent_of(f.cells_a[0]).unwrap(),
    ] {
        for step in [
            TransactionStep::InsertTableRow {
                table: target,
                index: 0,
            },
            TransactionStep::InsertTableColumn {
                table: target,
                index: 0,
            },
        ] {
            assert!(
                Transaction::new(TransactionOrigin::UserInput)
                    .with_step(step)
                    .apply(&f.document)
                    .is_err()
            );
        }
    }
    for step in [
        TransactionStep::InsertTableRow {
            table: f.table_a,
            index: 0,
        },
        TransactionStep::InsertTableColumn {
            table: f.table_a,
            index: 0,
        },
    ] {
        assert!(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::RemoveNode { node: f.cells_a[0] })
                .with_step(step)
                .apply(&f.document)
                .is_err()
        );
    }
    assert_eq!(children_of(&f.document, f.table_a).len(), 2);
}

#[test]
fn reversed_partial_rectangle_backspace_preserves_neighbors_and_selection_on_undo() {
    let f = fixture();
    let mut session = session_at(&f.document, f.texts_a[3], 0);
    session
        .set_cell_range_selection(f.cells_a[3], f.cells_a[1])
        .unwrap();
    let before = session.selection();
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, f.texts_a[0]), "a1");
    assert_eq!(text_of(&session, f.texts_a[2]), "a2");
    assert_eq!(
        session.clipboard_slice().unwrap().unwrap().plain_text(),
        "\n"
    );
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn wire_accepts_legacy_tables_and_rejects_invalid_versions_shapes_and_stale_body() {
    use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};
    let f = fixture();
    let mut session = session_at(&f.document, f.intro, 0);
    session
        .set_cell_range_selection(f.cells_a[0], f.cells_a[3])
        .unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    let wire = encode_metadata(&slice).unwrap();
    let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(value["version"], 6);
    let mut legacy = value.clone();
    legacy["version"] = 5.into();
    legacy["roots"][0]["content"]["value"]
        .as_object_mut()
        .unwrap()
        .remove("row_attrs");
    assert_eq!(
        decode_metadata(slice.plain_text(), &legacy.to_string()).unwrap(),
        slice
    );
    for version in [0, 3, 4, 7, 999] {
        let mut bad = value.clone();
        bad["version"] = version.into();
        assert!(decode_metadata(slice.plain_text(), &bad.to_string()).is_none());
    }
    for field in ["rows", "row_attrs"] {
        let mut bad = value.clone();
        bad["roots"][0]["content"]["value"][field]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(decode_metadata(slice.plain_text(), &bad.to_string()).is_none());
    }
    let mut missing = value.clone();
    missing["roots"][0]["content"]["value"]
        .as_object_mut()
        .unwrap()
        .remove("row_attrs");
    assert!(decode_metadata(slice.plain_text(), &missing.to_string()).is_none());
    assert!(decode_metadata("stale", &wire).is_none());
}
