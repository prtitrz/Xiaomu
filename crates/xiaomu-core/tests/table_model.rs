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

#[test]
fn insert_table_row_step_appends_one_uniform_row() {
    use xiaomu_core::mapping::StepMap;

    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "前");
    let first = cell(&mut builder, "a");
    let second = cell(&mut builder, "b");
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
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
            NodeContent::children([intro, table]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableRow { table, index: 1 })
        .apply_with_changes(&document)
        .unwrap();
    let snapshot = applied.document();
    snapshot.validate().unwrap();

    let rows = snapshot
        .node(table)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec();
    assert_eq!(rows.len(), 2);
    for appended_row in &rows {
        let cells = snapshot
            .node(*appended_row)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec();
        assert_eq!(cells.len(), 2, "the new row mirrors the column count");
    }

    // The step map reports the new row's FIRST cell paragraph as the caret
    // target, and it really lives inside the appended row.
    let inserted = applied
        .changes()
        .steps()
        .iter()
        .rev()
        .find_map(|step| match step {
            StepMap::NodeInserted { inserted, .. } => Some(*inserted),
            _ => None,
        })
        .expect("one inserted node");
    let first_cell = snapshot.parent_of(inserted).unwrap();
    assert_eq!(snapshot.parent_of(first_cell).unwrap(), rows[1]);
    let first_cell_blocks = snapshot
        .node(first_cell)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec();
    assert_eq!(first_cell_blocks, vec![inserted]);

    // The inverse removes exactly the appended row and validates.
    let inverse = applied.inverse().clone();
    let mut undo_transaction = Transaction::new(TransactionOrigin::UserInput);
    for step in inverse.steps() {
        undo_transaction.push_step(step.clone());
    }
    let undone = undo_transaction.apply_with_changes(snapshot).unwrap();
    undone.document().validate().unwrap();
    let rows = undone
        .document()
        .node(table)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec();
    assert_eq!(rows, vec![row]);
    assert!(undone.document().node(inserted).is_none());

    // A non-table target fails closed.
    assert!(
        Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertTableRow {
                table: intro,
                index: 0,
            })
            .apply_with_changes(&document)
            .is_err()
    );

    // An out-of-range row index fails closed.
    assert!(
        Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertTableRow { table, index: 9 })
            .apply_with_changes(&document)
            .is_err()
    );
}

#[test]
fn insert_table_column_step_adds_one_cell_per_row() {
    use xiaomu_core::mapping::StepMap;

    let mut builder = NodeStoreBuilder::new();
    let first = cell(&mut builder, "a");
    let second = cell(&mut builder, "b");
    let third = cell(&mut builder, "c");
    let fourth = cell(&mut builder, "d");
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    let row2 = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([third, fourth]),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row, row2]),
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

    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableColumn { table, index: 1 })
        .apply_with_changes(&document)
        .unwrap();
    let snapshot = applied.document();
    snapshot.validate().unwrap();

    let row_cells = snapshot
        .node(row)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec();
    let row2_cells = snapshot
        .node(row2)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec();
    assert_eq!(row_cells.len(), 3, "every row gains one cell");
    assert_eq!(row2_cells.len(), 3);
    assert_eq!(row_cells[0], first, "cells keep their identities");
    assert_eq!(row2_cells[0], third);

    // The step map reports the first row's inserted cell as the caret
    // target.
    let inserted = applied
        .changes()
        .steps()
        .iter()
        .rev()
        .find_map(|step| match step {
            StepMap::NodeInserted { inserted, .. } => Some(*inserted),
            _ => None,
        })
        .expect("one inserted node");
    assert_eq!(snapshot.parent_of(inserted).unwrap(), row);

    // The inverse removes one cell per row and validates.
    let inverse = applied.inverse().clone();
    let mut undo_transaction = Transaction::new(TransactionOrigin::UserInput);
    for step in inverse.steps() {
        undo_transaction.push_step(step.clone());
    }
    let undone = undo_transaction.apply_with_changes(snapshot).unwrap();
    undone.document().validate().unwrap();
    assert_eq!(
        undone
            .document()
            .node(row)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec(),
        vec![first, second]
    );
    assert!(undone.document().node(inserted).is_none());

    // Out-of-range column indexes fail closed.
    assert!(
        Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertTableColumn { table, index: 3 })
            .apply_with_changes(&document)
            .is_err()
    );
}
