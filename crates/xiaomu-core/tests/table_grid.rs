//! Header identity, preservation, geometry, resource, and atomicity regressions.

use xiaomu_core::document::{
    AttrValue, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TABLE_MAX_LOGICAL_SLOTS,
    TableAttribute, TableCellAttrs, TableRect, XiaomuDocument,
};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_core::{Error, Result};

fn attrs(values: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        values
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
    .unwrap()
}

fn spans(colspan: i64, rowspan: i64) -> NodeAttrs {
    attrs(&[
        ("colspan", AttrValue::Integer(colspan)),
        ("rowspan", AttrValue::Integer(rowspan)),
    ])
}

fn block(builder: &mut NodeStoreBuilder) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap()
}

fn cell(builder: &mut NodeStoreBuilder, kind: NodeKind, attrs: NodeAttrs) -> NodeId {
    let paragraph = block(builder);
    builder
        .insert(kind, attrs, NodeContent::children([paragraph]))
        .unwrap()
}

fn table(builder: &mut NodeStoreBuilder, rows: &[Vec<NodeId>]) -> NodeId {
    let rows: Vec<_> = rows
        .iter()
        .map(|cells| {
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(cells.iter().copied()),
                )
                .unwrap()
        })
        .collect();
    builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap()
}

fn finish(mut builder: NodeStoreBuilder, children: &[NodeId]) -> Result<XiaomuDocument> {
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children.iter().copied()),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish())
}

fn geometry(rows: &[&[(i64, i64)]]) -> Result<(XiaomuDocument, NodeId)> {
    let mut builder = NodeStoreBuilder::new();
    let rows: Vec<_> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|(cols, rows)| cell(&mut builder, NodeKind::TableCell, spans(*cols, *rows)))
                .collect()
        })
        .collect();
    let table = table(&mut builder, &rows);
    Ok((finish(builder, &[table])?, table))
}

#[test]
fn known_attributes_keep_presence_and_unknown_values() {
    let empty = NodeAttrs::empty();
    let read = TableCellAttrs::read(&empty).unwrap();
    assert_eq!(read.colspan(), TableAttribute::Missing);
    assert_eq!(read.rowspan(), TableAttribute::Missing);
    assert_eq!(read.colwidth(), TableAttribute::Missing);
    assert_eq!(read.effective_colspan(), Ok(1));
    assert_eq!(read.effective_rowspan(), Ok(1));
    let source = attrs(&[
        ("colspan", AttrValue::Integer(2)),
        (
            "colwidth",
            AttrValue::List(vec![AttrValue::Integer(0), AttrValue::Integer(120)]),
        ),
        ("align", AttrValue::String("host-value".into())),
        ("backgroundColor", AttrValue::Null),
        (
            "extension",
            AttrValue::List(vec![AttrValue::Null, AttrValue::Bool(true)]),
        ),
    ]);
    let original = source.clone();
    let read = TableCellAttrs::read(&source).unwrap();
    read.validate_geometry().unwrap();
    let TableAttribute::Value(widths) = read.colwidth() else {
        panic!("widths present")
    };
    assert_eq!(widths.iter().collect::<Vec<_>>(), vec![0, 120]);
    assert_eq!(source, original);
    let null = attrs(&[("colspan", AttrValue::Null), ("colwidth", AttrValue::Null)]);
    let read = TableCellAttrs::read(&null).unwrap();
    assert_eq!(read.colspan(), TableAttribute::Null);
    assert_eq!(read.colwidth(), TableAttribute::Null);
    assert_eq!(read.effective_colspan(), Err(Error::InvalidTableAttrs));
}

#[test]
fn invalid_known_attrs_are_rejected_for_both_cell_kinds() {
    let cases = [
        ("colspan", AttrValue::Null),
        ("rowspan", AttrValue::Null),
        ("colspan", AttrValue::Integer(0)),
        ("rowspan", AttrValue::Integer(-1)),
        ("colspan", AttrValue::Bool(true)),
        ("rowspan", AttrValue::String("2".into())),
        ("rowspan", AttrValue::String("1.5".into())),
        ("colwidth", AttrValue::Integer(2)),
        ("colwidth", AttrValue::List(vec![])),
        ("colwidth", AttrValue::List(vec![AttrValue::Integer(-1)])),
        ("colwidth", AttrValue::List(vec![AttrValue::Null])),
        ("colwidth", AttrValue::List(vec![AttrValue::Bool(true)])),
        (
            "colwidth",
            AttrValue::List(vec![AttrValue::Integer(1), AttrValue::Integer(2)]),
        ),
    ];
    for kind in [NodeKind::TableCell, NodeKind::TableHeader] {
        for (key, value) in &cases {
            let mut builder = NodeStoreBuilder::new();
            let paragraph = block(&mut builder);
            assert_eq!(
                builder.insert(
                    kind.clone(),
                    attrs(&[(key, value.clone())]),
                    NodeContent::children([paragraph])
                ),
                Err(Error::InvalidTableAttrs),
                "{key}: {value:?}"
            );
        }
    }
}

#[test]
fn mixed_spans_ragged_rows_and_header_origins_are_logical() {
    let mut builder = NodeStoreBuilder::new();
    let a = cell(&mut builder, NodeKind::TableHeader, spans(2, 2));
    let b = cell(&mut builder, NodeKind::TableCell, spans(1, 1));
    let c = cell(&mut builder, NodeKind::TableHeader, spans(1, 1));
    let d = cell(&mut builder, NodeKind::TableCell, spans(1, 1));
    let e = cell(&mut builder, NodeKind::TableCell, spans(2, 1));
    let table = table(&mut builder, &[vec![a, b], vec![c], vec![d, e]]);
    let doc = finish(builder, &[table]).unwrap();
    let grid = doc.table_grid(table).unwrap();
    assert_eq!((grid.rows(), grid.columns()), (3, 3));
    assert!(grid.has_spans());
    assert_eq!(
        grid.origins().map(|cell| cell.cell()).collect::<Vec<_>>(),
        vec![a, b, c, d, e]
    );
    assert_eq!(grid.slot(1, 0), Some(a));
    assert_eq!(grid.slot(1, 1), Some(a));
    assert_eq!(grid.slot(1, 2), Some(c));
    assert_eq!(grid.slot(3, 0), None);
    assert_eq!(grid.slot(0, usize::MAX), None);
    let placement = grid.placement(c).unwrap();
    assert_eq!(
        (
            placement.row(),
            placement.column(),
            placement.physical_index()
        ),
        (1, 2, 0)
    );
    assert_eq!(placement.row_id(), grid.row_id(1).unwrap());
    let rect = grid.rect_between(a, c).unwrap();
    assert!(grid.is_closed_rect(rect));
    assert_eq!(grid.unique_cells_in(rect).unwrap(), vec![a, b, c]);
    let partial = TableRect::new(1, 1, 2, 3).unwrap();
    assert!(!grid.is_closed_rect(partial));
    assert_eq!(grid.unique_cells_in(partial).unwrap(), vec![a, c]);
    assert_eq!(
        grid.unique_cells_in(TableRect::new(0, 0, 4, 3).unwrap()),
        Err(Error::InvalidSelection)
    );
    assert_eq!(TableRect::new(0, 0, 0, 1), Err(Error::InvalidSelection));
}

#[test]
fn horizontal_vertical_and_fully_covered_empty_rows_validate() {
    for rows in [
        vec![vec![(2, 1)], vec![(1, 1), (1, 1)]],
        vec![vec![(1, 2), (1, 1)], vec![(1, 1)]],
        vec![vec![(2, 2)], vec![]],
    ] {
        let refs: Vec<_> = rows.iter().map(Vec::as_slice).collect();
        let (doc, table) = geometry(&refs).unwrap();
        let grid = doc.table_grid(table).unwrap();
        assert_eq!((grid.rows(), grid.columns()), (2, 2));
        doc.validate().unwrap();
    }
}

#[test]
fn holes_collision_overspan_and_empty_uncovered_rows_fail() {
    let cases = [
        vec![vec![(2, 1)], vec![(1, 1)]],
        vec![vec![(2, 1)], vec![]],
        vec![vec![], vec![(1, 1)]],
        vec![vec![(1, 3)], vec![]],
        vec![vec![(2, 1)], vec![(3, 1)]],
        // A free two-slot run exists farther right, but skipping the first
        // uncovered slot would silently repair the invalid collision.
        vec![vec![(1, 1), (1, 2), (2, 1)], vec![(2, 1)]],
    ];
    for rows in cases {
        let refs: Vec<_> = rows.iter().map(Vec::as_slice).collect();
        assert_eq!(
            geometry(&refs).unwrap_err(),
            Error::InvalidTableStructure,
            "{rows:?}"
        );
    }
}

#[test]
fn nested_tables_have_independent_grids() {
    let mut builder = NodeStoreBuilder::new();
    let inner_cell = cell(&mut builder, NodeKind::TableHeader, spans(3, 2));
    let inner = table(&mut builder, &[vec![inner_cell], vec![]]);
    let outer_cell = builder
        .insert(
            NodeKind::TableCell,
            spans(2, 1),
            NodeContent::children([inner]),
        )
        .unwrap();
    let outer = table(&mut builder, &[vec![outer_cell]]);
    let doc = finish(builder, &[outer]).unwrap();
    assert_eq!(doc.table_grid(outer).unwrap().columns(), 2);
    assert_eq!(doc.table_grid(inner).unwrap().columns(), 3);
    assert_eq!(doc.table_grid(inner).unwrap().slot(1, 2), Some(inner_cell));
}

#[test]
fn huge_spans_and_snapshot_wide_nested_budgets_fail_before_allocation() {
    assert_eq!(
        geometry(&[&[(i64::MAX, 1)]]).unwrap_err(),
        Error::TableResourceLimit
    );
    assert_eq!(
        geometry(&[&[(i64::MAX, 1), (i64::MAX, 1), (i64::MAX, 1)]]).unwrap_err(),
        Error::TableResourceLimit
    );
    let width = (TABLE_MAX_LOGICAL_SLOTS / 2 + 1) as i64;
    for nested in [false, true] {
        let mut builder = NodeStoreBuilder::new();
        let a = cell(&mut builder, NodeKind::TableCell, spans(width, 1));
        let first = table(&mut builder, &[vec![a]]);
        let b = if nested {
            builder
                .insert(
                    NodeKind::TableCell,
                    spans(width, 1),
                    NodeContent::children([first]),
                )
                .unwrap()
        } else {
            cell(&mut builder, NodeKind::TableCell, spans(width, 1))
        };
        let second = table(&mut builder, &[vec![b]]);
        let roots = if nested {
            vec![second]
        } else {
            vec![first, second]
        };
        assert_eq!(
            finish(builder, &roots).unwrap_err(),
            Error::TableResourceLimit
        );
    }
}

#[test]
fn header_kind_attrs_and_whole_subtree_undo_are_exact() {
    let mut builder = NodeStoreBuilder::new();
    let metadata = attrs(&[
        ("backgroundColor", AttrValue::Null),
        ("unknown", AttrValue::String("keep".into())),
    ]);
    let header = cell(&mut builder, NodeKind::TableHeader, metadata.clone());
    let table = table(&mut builder, &[vec![header]]);
    let doc = finish(builder, &[table]).unwrap();
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeKind {
            node: header,
            kind: NodeKind::TableCell,
        })
        .apply_with_changes(&doc)
        .unwrap();
    let restored = applied.inverse().apply(applied.document()).unwrap();
    assert_eq!(restored.store(), doc.store());
    assert_eq!(restored.node(header).unwrap().attrs(), &metadata);
    let removed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: table })
        .apply_with_changes(&doc)
        .unwrap();
    let restored = removed.inverse().apply(removed.document()).unwrap();
    assert_eq!(restored.store(), doc.store());
}

#[test]
fn legacy_spanning_row_column_steps_reject_without_changes() {
    let (doc, table) = geometry(&[&[(2, 2)], &[]]).unwrap();
    let original = doc.store().clone();
    let revision = doc.revision();
    for step in [
        TransactionStep::InsertTableRow { table, index: 1 },
        TransactionStep::InsertTableColumn { table, index: 1 },
    ] {
        assert_eq!(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(step)
                .apply(&doc)
                .unwrap_err(),
            Error::UnsupportedTableOperation
        );
        assert_eq!(doc.store(), &original);
        assert_eq!(doc.revision(), revision);
    }
}

#[test]
fn unit_headers_survive_legacy_insertions_and_new_cells_keep_body_defaults() {
    let mut builder = NodeStoreBuilder::new();
    let header = cell(&mut builder, NodeKind::TableHeader, NodeAttrs::empty());
    let table = table(&mut builder, &[vec![header]]);
    let doc = finish(builder, &[table]).unwrap();
    for step in [
        TransactionStep::InsertTableRow { table, index: 1 },
        TransactionStep::InsertTableColumn { table, index: 1 },
    ] {
        let applied = Transaction::new(TransactionOrigin::UserInput)
            .with_step(step)
            .apply_with_changes(&doc)
            .unwrap();
        assert_eq!(
            applied.document().node(header).unwrap().kind(),
            &NodeKind::TableHeader
        );
        let grid = applied.document().table_grid(table).unwrap();
        let new = grid
            .origins()
            .find(|cell| cell.cell() != header)
            .unwrap()
            .cell();
        assert_eq!(
            applied.document().node(new).unwrap().kind(),
            &NodeKind::TableCell
        );
        assert!(applied.document().node(new).unwrap().attrs().is_empty());
        assert_eq!(
            applied.inverse().apply(applied.document()).unwrap().store(),
            doc.store()
        );
    }
}

#[test]
fn raw_transaction_checks_only_final_geometry_and_rolls_back_failure() {
    let (doc, table) = geometry(&[&[(1, 1), (1, 1)], &[(1, 1), (1, 1)]]).unwrap();
    let grid = doc.table_grid(table).unwrap();
    let a = grid.slot(0, 0).unwrap();
    let b = grid.slot(0, 1).unwrap();
    let original = doc.store().clone();
    let invalid = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeKind {
            node: a,
            kind: NodeKind::TableHeader,
        })
        .with_step(TransactionStep::SetNodeAttrs {
            node: a,
            attrs: spans(2, 1),
        });
    assert_eq!(
        invalid.apply(&doc).unwrap_err(),
        Error::InvalidTableStructure
    );
    assert_eq!(doc.store(), &original);
    assert_eq!(doc.revision().as_u64(), 0);
    let valid = invalid
        .with_step(TransactionStep::RemoveNode { node: b })
        .apply_with_changes(&doc)
        .unwrap();
    assert_eq!(
        valid.document().table_grid(table).unwrap().slot(0, 1),
        Some(a)
    );
    assert_eq!(
        valid.inverse().apply(valid.document()).unwrap().store(),
        doc.store()
    );
}

#[test]
fn huge_insert_table_dimensions_fail_without_touching_snapshot() {
    let doc = finish(NodeStoreBuilder::new(), &[]).unwrap();
    for (rows, columns) in [
        (usize::MAX, usize::MAX),
        (1, TABLE_MAX_LOGICAL_SLOTS + 1),
        (100_001, 1),
    ] {
        assert!(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::InsertTable {
                    parent: doc.root(),
                    index: 0,
                    rows,
                    columns,
                })
                .apply(&doc)
                .is_err()
        );
        assert_eq!(doc.node_count(), 1);
        assert_eq!(doc.revision().as_u64(), 0);
    }
}

#[test]
fn body_and_header_cells_are_rejected_under_ordinary_block_parents() {
    for parent_kind in [
        NodeKind::Document,
        NodeKind::Quote,
        NodeKind::ListItem,
        NodeKind::TableCell,
        NodeKind::TableHeader,
    ] {
        for cell_kind in [NodeKind::TableCell, NodeKind::TableHeader] {
            let mut builder = NodeStoreBuilder::new();
            let child = cell(&mut builder, cell_kind, NodeAttrs::empty());
            assert_eq!(
                builder.insert(
                    parent_kind.clone(),
                    NodeAttrs::empty(),
                    NodeContent::children([child])
                ),
                Err(Error::InvalidChildKind)
            );
        }
    }
}

#[test]
fn header_content_shape_and_nonempty_final_content_match_body_cells() {
    let mut builder = NodeStoreBuilder::new();
    assert_eq!(
        builder.insert(
            NodeKind::TableHeader,
            NodeAttrs::empty(),
            NodeContent::Atomic
        ),
        Err(Error::InvalidNodeContent)
    );
    let empty = builder
        .insert(
            NodeKind::TableHeader,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let table = table(&mut builder, &[vec![empty]]);
    assert_eq!(
        finish(builder, &[table]).unwrap_err(),
        Error::InvalidTableStructure
    );
}

#[test]
fn missing_and_null_widths_preserve_distinct_valid_cell_attributes() {
    let mut builder = NodeStoreBuilder::new();
    let missing = cell(&mut builder, NodeKind::TableHeader, NodeAttrs::empty());
    let null_attrs = attrs(&[("colwidth", AttrValue::Null)]);
    let null = cell(&mut builder, NodeKind::TableCell, null_attrs.clone());
    let table = table(&mut builder, &[vec![missing, null]]);
    let doc = finish(builder, &[table]).unwrap();
    assert_eq!(doc.node(missing).unwrap().attrs().get("colwidth"), None);
    assert_eq!(doc.node(null).unwrap().attrs(), &null_attrs);
    assert!(!doc.table_grid(table).unwrap().has_spans());
}
