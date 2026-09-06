//! P5.1 table canonical model regressions.
//!
//! Tables are ordinary tree nodes whose structural invariants (at least one
//! row, uniform column count, non-empty cells) are held by canonical
//! validation, not by frontend convention.

use std::collections::BTreeSet;

use xiaomu_core::Error;
use xiaomu_core::document::{
    InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn cell(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    let paragraph = paragraph(builder, text);
    builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap()
}

/// Builds one valid 1×1 table and returns `(table, row, cell)`.
fn minimal_table(builder: &mut NodeStoreBuilder) -> (NodeId, NodeId, NodeId) {
    let cell = cell(builder, "a");
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
    (table, row, cell)
}

#[test]
fn uniform_table_validates_and_nests_like_other_blocks() {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "前");
    let (table, row, cell) = minimal_table(&mut builder);
    let (table2, _, _) = minimal_table(&mut builder);
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([table2]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table, quote]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    assert_eq!(document.parent_of(row), Some(table));
    assert_eq!(document.parent_of(cell), Some(row));
    assert_eq!(document.parent_of(table), Some(root));
    // A nested table inside a quote container is a regular block.
    assert_eq!(document.parent_of(table2), Some(quote));
}

#[test]
fn ragged_or_empty_tables_fail_closed() {
    // Rows with different cell counts.
    let mut builder = NodeStoreBuilder::new();
    let cell_a = cell(&mut builder, "a");
    let (row_a, _) = {
        let row = builder
            .insert(
                NodeKind::TableRow,
                NodeAttrs::empty(),
                NodeContent::children([cell_a]),
            )
            .unwrap();
        (row, ())
    };
    let cell_b1 = cell(&mut builder, "b");
    let cell_b2 = cell(&mut builder, "c");
    let row_b = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell_b1, cell_b2]),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row_a, row_b]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    match XiaomuDocument::new(root, builder.finish()) {
        Err(Error::InvalidTableStructure) => {}
        other => panic!("expected Error::InvalidTableStructure, got {other:?}"),
    }

    // Table without rows.
    let mut builder = NodeStoreBuilder::new();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    match XiaomuDocument::new(root, builder.finish()) {
        Err(Error::InvalidTableStructure) => {}
        other => panic!("expected Error::InvalidTableStructure, got {other:?}"),
    }

    // Row without cells.
    let mut builder = NodeStoreBuilder::new();
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([]),
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
    match XiaomuDocument::new(root, builder.finish()) {
        Err(Error::InvalidTableStructure) => {}
        other => panic!("expected Error::InvalidTableStructure, got {other:?}"),
    }

    // Cell without a child block.
    let mut builder = NodeStoreBuilder::new();
    let empty_cell = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([empty_cell]),
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
    match XiaomuDocument::new(root, builder.finish()) {
        Err(Error::InvalidTableStructure) => {}
        other => panic!("expected Error::InvalidTableStructure, got {other:?}"),
    }
}

#[test]
fn invalid_table_nesting_fails_closed() {
    // The builder rejects invalid parent/child pairs before any document
    // validation runs.
    let mut builder = NodeStoreBuilder::new();
    let row_cell = cell(&mut builder, "a");
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([row_cell]),
        )
        .unwrap();
    match builder.insert(
        NodeKind::Document,
        NodeAttrs::empty(),
        NodeContent::children([row]),
    ) {
        Err(Error::InvalidChildKind) => {}
        other => panic!("expected InvalidChildKind, got {other:?}"),
    }

    let mut builder = NodeStoreBuilder::new();
    let table_cell = cell(&mut builder, "a");
    match builder.insert(
        NodeKind::Table,
        NodeAttrs::empty(),
        NodeContent::children([table_cell]),
    ) {
        Err(Error::InvalidChildKind) => {}
        other => panic!("expected InvalidChildKind, got {other:?}"),
    }

    let mut builder = NodeStoreBuilder::new();
    let stray_cell = cell(&mut builder, "a");
    let stray_row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([stray_cell]),
        )
        .unwrap();
    match builder.insert(
        NodeKind::Quote,
        NodeAttrs::empty(),
        NodeContent::children([stray_row]),
    ) {
        Err(Error::InvalidChildKind) => {}
        other => panic!("expected InvalidChildKind, got {other:?}"),
    }
    let _ = BTreeSet::<NodeId>::new();
}

#[test]
fn insert_table_step_allocates_a_whole_valid_subtree() {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "前");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTable {
            parent: root,
            index: 1,
            rows: 2,
            columns: 3,
        })
        .apply_with_changes(&document)
        .unwrap();
    let snapshot = applied.document();
    snapshot.validate().unwrap();

    let children = snapshot
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec();
    assert_eq!(children.len(), 2);
    let table = children[1];
    let rows = snapshot
        .node(table)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        let cells = snapshot
            .node(*row)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec();
        assert_eq!(cells.len(), 3);
        for cell in &cells {
            let blocks = snapshot
                .node(*cell)
                .unwrap()
                .content()
                .as_children()
                .unwrap();
            assert_eq!(blocks.len(), 1);
        }
    }

    // The inverse removes the whole subtree and validates.
    let inverse = applied.inverse().clone();
    let mut undo_transaction = Transaction::new(TransactionOrigin::UserInput);
    for step in inverse.steps() {
        undo_transaction.push_step(step.clone());
    }
    let undone = undo_transaction.apply_with_changes(snapshot).unwrap();
    undone.document().validate().unwrap();
    assert!(undone.document().node(table).is_none());

    // Degenerate dimensions fail closed.
    assert!(
        Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertTable {
                parent: root,
                index: 1,
                rows: 0,
                columns: 3,
            })
            .apply_with_changes(&document)
            .is_err()
    );
}
