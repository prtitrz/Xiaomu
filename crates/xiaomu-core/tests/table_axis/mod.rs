//! Logical-axis property and regression matrix, activated by its test target.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use xiaomu_core::Error;
use xiaomu_core::document::{
    AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TableCellAttrs, TextRun, XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition, StepMap};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap, NodeSelection};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{
    AppliedTransaction, Transaction, TransactionOrigin, TransactionStep,
};

struct Fixture {
    document: XiaomuDocument,
    table: NodeId,
    rows: Vec<NodeId>,
    named: BTreeMap<&'static str, NodeId>,
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

fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let mut named = BTreeMap::new();
    let specs = vec![
        vec![("a", 3, 2), ("b", 1, 1), ("c", 2, 1)],
        vec![("d", 1, 1)],
        vec![("e", 1, 1), ("f", 1, 1)],
        vec![("g", 1, 1), ("h", 1, 1), ("i", 1, 1), ("j", 1, 1)],
    ];
    let mut rows = Vec::new();
    for row_specs in specs {
        let mut cells = Vec::new();
        for (label, rowspan, colspan) in row_specs {
            let p = paragraph(&mut builder, label);
            let mut blocks = vec![p];
            if label == "a" {
                let nested_p = paragraph(&mut builder, "nested");
                let nested_cell = builder
                    .insert(
                        NodeKind::TableCell,
                        NodeAttrs::empty(),
                        NodeContent::children([nested_p]),
                    )
                    .unwrap();
                let nested_row = builder
                    .insert(
                        NodeKind::TableRow,
                        NodeAttrs::empty(),
                        NodeContent::children([nested_cell]),
                    )
                    .unwrap();
                let nested = builder
                    .insert(
                        NodeKind::Table,
                        NodeAttrs::empty(),
                        NodeContent::children([nested_row]),
                    )
                    .unwrap();
                blocks.push(nested);
                named.insert("nested", nested);
                named.insert("a-text", p);
            }
            let mut attrs = BTreeMap::from([
                ("sentinel".into(), AttrValue::String(label.into())),
                ("background".into(), AttrValue::Null),
            ]);
            if rowspan > 1 {
                attrs.insert("rowspan".into(), AttrValue::Integer(rowspan));
            }
            if colspan > 1 {
                attrs.insert("colspan".into(), AttrValue::Integer(colspan));
            }
            attrs.insert(
                "colwidth".into(),
                AttrValue::List(
                    (0..colspan)
                        .map(|column| AttrValue::Integer(100 + column * 20))
                        .collect(),
                ),
            );
            let kind = if label == "a" || label == "c" {
                NodeKind::TableHeader
            } else {
                NodeKind::TableCell
            };
            let cell = builder
                .insert(
                    kind,
                    NodeAttrs::new(attrs).unwrap(),
                    NodeContent::children(blocks),
                )
                .unwrap();
            cells.push(cell);
            named.insert(label, cell);
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
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows.clone()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        rows,
        named,
    }
}

fn apply(document: &XiaomuDocument, step: TransactionStep) -> AppliedTransaction {
    Transaction::new(TransactionOrigin::UserInput)
        .with_step(step)
        .apply_with_changes(document)
        .unwrap()
}

fn descendants(document: &XiaomuDocument, root: NodeId) -> BTreeSet<NodeId> {
    let mut nodes = BTreeSet::new();
    let mut pending = VecDeque::from([root]);
    while let Some(id) = pending.pop_front() {
        if !nodes.insert(id) {
            continue;
        }
        let node = document.node(id).unwrap();
        if let Some(children) = node.content().as_children() {
            pending.extend(children);
        }
        if let Some(inline) = node.content().as_inline() {
            pending.extend(inline.atoms().iter().map(|atom| atom.atom()));
        }
    }
    nodes
}

fn round_trip(before: &XiaomuDocument, applied: &AppliedTransaction) {
    applied.document().validate().unwrap();
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undo.document().store(), before.store());
    assert_eq!(undo.document().root(), before.root());
    let redo = undo.inverse().apply_with_changes(undo.document()).unwrap();
    assert_eq!(redo.document().store(), applied.document().store());
}

#[test]
fn insert_at_every_row_boundary_preserves_origins_and_extends_only_crossing_spans() {
    let f = fixture();
    let before = f.document.table_grid(f.table).unwrap();
    for index in 0..=before.rows() {
        let applied = apply(
            &f.document,
            TransactionStep::InsertTableRowLogical {
                table: f.table,
                index,
                cell_kinds: vec![
                    NodeKind::TableHeader,
                    NodeKind::TableCell,
                    NodeKind::TableHeader,
                    NodeKind::TableCell,
                ],
            },
        );
        let after = applied.document().table_grid(f.table).unwrap();
        assert_eq!(after.rows(), before.rows() + 1);
        assert_eq!(after.columns(), before.columns());
        for old in before.origins() {
            let next = after.placement(old.cell()).unwrap();
            assert_eq!(next.row(), old.row() + usize::from(old.row() >= index));
            assert_eq!(next.column(), old.column());
            assert_eq!(next.colspan(), old.colspan());
            assert_eq!(
                next.rowspan(),
                old.rowspan() + usize::from(old.row() < index && old.row() + old.rowspan() > index)
            );
            for id in descendants(&f.document, old.cell())
                .into_iter()
                .filter(|id| *id != old.cell())
            {
                assert_eq!(applied.document().node(id), f.document.node(id));
            }
        }
        for cell in after.origins().filter(|cell| cell.row() == index) {
            if f.document.node(cell.cell()).is_none() {
                let node = applied.document().node(cell.cell()).unwrap();
                let expected = if cell.column() % 2 == 0 {
                    NodeKind::TableHeader
                } else {
                    NodeKind::TableCell
                };
                assert_eq!(node.kind(), &expected);
                assert!(node.attrs().is_empty());
            }
        }
        round_trip(&f.document, &applied);
    }
}

#[test]
fn insert_at_every_column_boundary_preserves_origins_and_splices_widths_once() {
    let f = fixture();
    let before = f.document.table_grid(f.table).unwrap();
    for index in 0..=before.columns() {
        let applied = apply(
            &f.document,
            TransactionStep::InsertTableColumnLogical {
                table: f.table,
                index,
                cell_kinds: vec![
                    NodeKind::TableHeader,
                    NodeKind::TableCell,
                    NodeKind::TableHeader,
                    NodeKind::TableCell,
                ],
            },
        );
        let after = applied.document().table_grid(f.table).unwrap();
        assert_eq!(after.columns(), before.columns() + 1);
        for old in before.origins() {
            let next = after.placement(old.cell()).unwrap();
            let crosses = old.column() < index && old.column() + old.colspan() > index;
            assert_eq!(
                next.column(),
                old.column() + usize::from(old.column() >= index)
            );
            assert_eq!(next.row(), old.row());
            assert_eq!(next.rowspan(), old.rowspan());
            assert_eq!(next.colspan(), old.colspan() + usize::from(crosses));
            let mut widths = match f
                .document
                .node(old.cell())
                .unwrap()
                .attrs()
                .get("colwidth")
                .unwrap()
            {
                AttrValue::List(widths) => widths.clone(),
                _ => unreachable!(),
            };
            if crosses {
                widths.insert(index - old.column(), AttrValue::Integer(0));
            }
            assert_eq!(
                applied
                    .document()
                    .node(old.cell())
                    .unwrap()
                    .attrs()
                    .get("colwidth"),
                Some(&AttrValue::List(widths))
            );
        }
        round_trip(&f.document, &applied);
    }
}

#[test]
fn every_proper_row_range_has_projected_geometry_and_exact_subtree_inverse() {
    let f = fixture();
    let before = f.document.table_grid(f.table).unwrap();
    for start in 0..before.rows() {
        for end in start + 1..=before.rows() {
            if end - start == before.rows() {
                continue;
            }
            let applied = apply(
                &f.document,
                TransactionStep::DeleteTableRowsLogical {
                    table: f.table,
                    start,
                    end,
                },
            );
            let after = applied.document().table_grid(f.table).unwrap();
            assert_eq!(after.rows(), before.rows() - (end - start));
            for old in before.origins() {
                let overlap = (old.row() + old.rowspan())
                    .min(end)
                    .saturating_sub(old.row().max(start));
                if overlap == old.rowspan() {
                    for id in descendants(&f.document, old.cell()) {
                        assert!(applied.document().node(id).is_none());
                    }
                } else {
                    let next = after.placement(old.cell()).unwrap();
                    let row = if old.row() >= end {
                        old.row() - (end - start)
                    } else if old.row() >= start {
                        start
                    } else {
                        old.row()
                    };
                    assert_eq!(next.row(), row);
                    assert_eq!(next.rowspan(), old.rowspan() - overlap);
                    assert_eq!(next.column(), old.column());
                    for id in descendants(&f.document, old.cell())
                        .into_iter()
                        .filter(|id| *id != old.cell())
                    {
                        assert_eq!(applied.document().node(id), f.document.node(id));
                    }
                }
            }
            round_trip(&f.document, &applied);
        }
    }
}

#[test]
fn every_proper_column_range_has_projected_geometry_and_exact_subtree_inverse() {
    let f = fixture();
    let before = f.document.table_grid(f.table).unwrap();
    for start in 0..before.columns() {
        for end in start + 1..=before.columns() {
            if end - start == before.columns() {
                continue;
            }
            let applied = apply(
                &f.document,
                TransactionStep::DeleteTableColumnsLogical {
                    table: f.table,
                    start,
                    end,
                },
            );
            let after = applied.document().table_grid(f.table).unwrap();
            assert_eq!(after.columns(), before.columns() - (end - start));
            for old in before.origins() {
                let overlap = (old.column() + old.colspan())
                    .min(end)
                    .saturating_sub(old.column().max(start));
                if overlap == old.colspan() {
                    for id in descendants(&f.document, old.cell()) {
                        assert!(applied.document().node(id).is_none());
                    }
                } else {
                    let next = after.placement(old.cell()).unwrap();
                    let column = if old.column() >= end {
                        old.column() - (end - start)
                    } else if old.column() >= start {
                        start
                    } else {
                        old.column()
                    };
                    assert_eq!(next.column(), column);
                    assert_eq!(next.colspan(), old.colspan() - overlap);
                    assert_eq!(next.row(), old.row());
                    assert_eq!(
                        applied.document().node(old.cell()).unwrap().content(),
                        f.document.node(old.cell()).unwrap().content()
                    );
                }
            }
            round_trip(&f.document, &applied);
        }
    }
}

#[test]
fn deleting_origin_row_moves_original_cells_and_maps_descendants_and_gaps() {
    let f = fixture();
    let a = f.named["a"];
    let c = f.named["c"];
    let text = f.named["a-text"];
    let applied = apply(
        &f.document,
        TransactionStep::DeleteTableRowsLogical {
            table: f.table,
            start: 0,
            end: 1,
        },
    );
    let after = applied.document();
    assert_eq!(after.parent_of(a), Some(f.rows[1]));
    assert_eq!(after.parent_of(c), Some(f.rows[1]));
    assert_eq!(
        after
            .node(f.rows[1])
            .unwrap()
            .content()
            .as_children()
            .unwrap(),
        &[a, f.named["d"], c]
    );
    let point = InlinePoint::new(text, TextOffset::ZERO, 0, CursorAffinity::After);
    assert_eq!(
        applied.changes().map_inline_point(point, MapBias::End),
        MappedPosition::Mapped(point)
    );
    assert_eq!(
        applied.changes().map_node_selection(NodeSelection::new(a)),
        MappedPosition::Mapped(NodeSelection::new(a))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_selection(NodeSelection::new(f.named["b"])),
        MappedPosition::Deleted
    );
    assert_eq!(
        applied
            .changes()
            .map_node_gap(NodeGap::new(a, 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(a, 1))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_gap(NodeGap::new(f.rows[1], 0), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(f.rows[1], 0))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_gap(NodeGap::new(f.rows[1], 0), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(f.rows[1], 1))
    );
    let undo = applied.inverse().apply_with_changes(after).unwrap();
    assert_eq!(
        undo.changes()
            .map_node_gap(NodeGap::new(f.rows[1], 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(f.rows[1], 0))
    );
    assert_eq!(
        undo.changes().map_inline_point(point, MapBias::End),
        MappedPosition::Mapped(point)
    );
}

#[test]
fn fully_covered_inserted_row_can_be_physically_empty_and_undo_exactly() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "full");
    let cell = builder
        .insert(
            NodeKind::TableHeader,
            NodeAttrs::new(BTreeMap::from([
                ("rowspan".into(), AttrValue::Integer(2)),
                ("colspan".into(), AttrValue::Integer(2)),
            ]))
            .unwrap(),
            NodeContent::children([p]),
        )
        .unwrap();
    let first = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell]),
        )
        .unwrap();
    let second = builder
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
            NodeContent::children([first, second]),
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
    let inserted = apply(
        &document,
        TransactionStep::InsertTableRowLogical {
            table,
            index: 1,
            cell_kinds: vec![NodeKind::TableCell; 2],
        },
    );
    let grid = inserted.document().table_grid(table).unwrap();
    assert!(
        inserted
            .document()
            .node(grid.row_id(1).unwrap())
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        inserted.document().store().len(),
        document.store().len() + 1
    );
    round_trip(&document, &inserted);
    let deleted = apply(
        &document,
        TransactionStep::DeleteTableRowsLogical {
            table,
            start: 0,
            end: 1,
        },
    );
    assert_eq!(deleted.document().parent_of(cell), Some(second));
    round_trip(&document, &deleted);
}

#[test]
fn rejects_delete_all_invalid_ranges_and_invalid_logical_kind_vectors_atomically() {
    let f = fixture();
    let original = f.document.store().clone();
    for step in [
        TransactionStep::DeleteTableRowsLogical {
            table: f.table,
            start: 0,
            end: 4,
        },
        TransactionStep::DeleteTableColumnsLogical {
            table: f.table,
            start: 0,
            end: 4,
        },
        TransactionStep::DeleteTableRowsLogical {
            table: f.table,
            start: 2,
            end: 2,
        },
        TransactionStep::DeleteTableColumnsLogical {
            table: f.table,
            start: 2,
            end: usize::MAX,
        },
        TransactionStep::InsertTableRowLogical {
            table: f.table,
            index: 1,
            cell_kinds: vec![NodeKind::TableCell; 3],
        },
        TransactionStep::InsertTableColumnLogical {
            table: f.table,
            index: 1,
            cell_kinds: vec![NodeKind::Paragraph; 4],
        },
        TransactionStep::InsertTableRowLogical {
            table: f.table,
            index: usize::MAX,
            cell_kinds: vec![NodeKind::TableCell; 4],
        },
    ] {
        assert_eq!(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(step)
                .apply_with_changes(&f.document)
                .unwrap_err(),
            Error::InvalidTransaction
        );
        assert_eq!(f.document.store(), &original);
    }
}

#[test]
fn old_unit_insertion_contract_still_refuses_spans() {
    let f = fixture();
    for step in [
        TransactionStep::InsertTableRow {
            table: f.table,
            index: 1,
        },
        TransactionStep::InsertTableColumn {
            table: f.table,
            index: 1,
        },
    ] {
        assert_eq!(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(step)
                .apply_with_changes(&f.document)
                .unwrap_err(),
            Error::UnsupportedTableOperation
        );
    }
}

#[test]
fn column_deletion_slices_widths_and_preserves_raw_null_other_attributes() {
    let f = fixture();
    let a = f.named["a"];
    let applied = apply(
        &f.document,
        TransactionStep::DeleteTableColumnsLogical {
            table: f.table,
            start: 0,
            end: 1,
        },
    );
    let attrs = applied.document().node(a).unwrap().attrs();
    assert_eq!(
        attrs.get("colwidth"),
        Some(&AttrValue::List(vec![AttrValue::Integer(120)]))
    );
    assert_eq!(attrs.get("background"), Some(&AttrValue::Null));
    assert_eq!(
        TableCellAttrs::read(attrs)
            .unwrap()
            .effective_colspan()
            .unwrap(),
        1
    );
    assert_eq!(
        applied.document().node(a).unwrap().kind(),
        &NodeKind::TableHeader
    );
    round_trip(&f.document, &applied);
}

#[test]
fn node_reparenting_maps_same_parent_indexes_in_post_removal_coordinates() {
    let f = fixture();
    let parent = f.rows[3];
    let moved = f.named["h"];
    let map = StepMap::NodeReparented {
        node: moved,
        old_parent: parent,
        old_index: 1,
        new_parent: parent,
        new_index: 3,
    };
    assert_eq!(
        map.map_node_gap(NodeGap::new(parent, 2), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(parent, 1))
    );
    assert_eq!(
        map.map_node_gap(NodeGap::new(parent, 4), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(parent, 3))
    );
    assert_eq!(
        map.map_node_gap(NodeGap::new(parent, 4), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(parent, 4))
    );
    assert_eq!(
        map.map_node_selection(NodeSelection::new(moved)),
        MappedPosition::Mapped(NodeSelection::new(moved))
    );
}

#[test]
fn stale_axis_inverse_rejects_reordered_table_rows() {
    let f = fixture();
    let inserted = apply(
        &f.document,
        TransactionStep::InsertTableRowLogical {
            table: f.table,
            index: 4,
            cell_kinds: vec![NodeKind::TableCell; 4],
        },
    );
    let next = apply(
        inserted.document(),
        TransactionStep::InsertTableRowLogical {
            table: f.table,
            index: 4,
            cell_kinds: vec![NodeKind::TableCell; 4],
        },
    );
    assert_eq!(
        inserted
            .inverse()
            .apply_with_changes(next.document())
            .unwrap_err(),
        Error::InvalidTransaction
    );
    round_trip(&f.document, &inserted);
}

#[test]
fn late_failure_after_axis_allocation_preserves_snapshot_and_allocator_sequence() {
    let f = fixture();
    let step = TransactionStep::InsertTableColumnLogical {
        table: f.table,
        index: 4,
        cell_kinds: vec![NodeKind::TableCell; 4],
    };
    let direct = apply(&f.document, step.clone());
    let failed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(step.clone())
        .with_step(TransactionStep::RemoveNode {
            node: f.document.root(),
        })
        .apply_with_changes(&f.document);
    assert_eq!(failed.unwrap_err(), Error::InvalidTransaction);
    let after_failure = apply(&f.document, step);
    assert_eq!(after_failure.document().store(), direct.document().store());
}
