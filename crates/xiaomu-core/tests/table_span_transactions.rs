//! Semantic span edits preserve canonical identities, geometry and exact undo.

use std::collections::BTreeMap;

use xiaomu_core::Error;
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TableRect, TextRun, XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition, StepMap};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap, NodeSelection};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{
    AppliedTransaction, Transaction, TransactionOrigin, TransactionStep,
};

fn attrs(values: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    )
    .unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    let content = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(content),
        )
        .unwrap()
}

fn cell(
    builder: &mut NodeStoreBuilder,
    kind: NodeKind,
    attributes: NodeAttrs,
    content: &[NodeId],
) -> NodeId {
    builder
        .insert(
            kind,
            attributes,
            NodeContent::children(content.iter().copied()),
        )
        .unwrap()
}

fn row(builder: &mut NodeStoreBuilder, cells: &[NodeId]) -> NodeId {
    builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells.iter().copied()),
        )
        .unwrap()
}

fn finish(mut builder: NodeStoreBuilder, rows: &[NodeId]) -> (XiaomuDocument, NodeId) {
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows.iter().copied()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), table)
}

fn apply(document: &XiaomuDocument, step: TransactionStep) -> AppliedTransaction {
    Transaction::new(TransactionOrigin::UserInput)
        .with_step(step)
        .apply_with_changes(document)
        .unwrap()
}

fn children(document: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    document
        .node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

fn round_trip(before: &XiaomuDocument, applied: &AppliedTransaction) {
    let undone = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undone.document().store(), before.store());
    assert_eq!(undone.document().root(), before.root());
    let redone = undone
        .inverse()
        .apply_with_changes(undone.document())
        .unwrap();
    assert_eq!(redone.document().store(), applied.document().store());
    assert_eq!(redone.document().root(), applied.document().root());
}

fn same_snapshot(left: &XiaomuDocument, right: &XiaomuDocument) {
    assert_eq!(left.store(), right.store());
    assert_eq!(left.root(), right.root());
    assert_eq!(left.revision(), right.revision());
    assert_eq!(left.version(), right.version());
}

#[test]
fn merge_preserves_rich_blocks_atoms_ids_top_left_kind_and_raw_attrs() {
    let mut builder = NodeStoreBuilder::new();
    let empty = paragraph(&mut builder, "");
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("hardBreak").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("\n").unwrap()),
        )
        .unwrap();
    let mixed = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("中🙂", MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let code = builder
        .insert(
            NodeKind::CodeBlock,
            attrs(&[("language", AttrValue::String("rust".into()))]),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("let x = 1;", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap();
    let quote_text = paragraph(&mut builder, "quote");
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([quote_text]),
        )
        .unwrap();
    let a_attrs = attrs(&[
        ("background", AttrValue::Null),
        (
            "extension",
            AttrValue::Object(BTreeMap::from([("x".into(), AttrValue::Bool(true))])),
        ),
        ("colwidth", AttrValue::List(vec![AttrValue::Integer(100)])),
    ]);
    let a = cell(&mut builder, NodeKind::TableHeader, a_attrs, &[empty]);
    let b = cell(
        &mut builder,
        NodeKind::TableCell,
        attrs(&[("colwidth", AttrValue::List(vec![AttrValue::Integer(120)]))]),
        &[mixed],
    );
    let c = cell(
        &mut builder,
        NodeKind::TableCell,
        NodeAttrs::empty(),
        &[code],
    );
    let d = cell(
        &mut builder,
        NodeKind::TableHeader,
        NodeAttrs::empty(),
        &[quote],
    );
    let r0 = row(&mut builder, &[a, b]);
    let r1 = row(&mut builder, &[c, d]);
    let (document, table) = finish(builder, &[r0, r1]);
    let applied = apply(
        &document,
        TransactionStep::MergeTableCells {
            table,
            rect: TableRect::new(0, 0, 2, 2).unwrap(),
        },
    );
    let after = applied.document();
    assert_eq!(children(after, a), vec![empty, mixed, code, quote]);
    assert_eq!(children(after, r0), vec![a]);
    assert!(children(after, r1).is_empty());
    assert_eq!(after.node(a).unwrap().kind(), &NodeKind::TableHeader);
    assert_eq!(
        after.node(a).unwrap().attrs().get("background"),
        Some(&AttrValue::Null)
    );
    assert_eq!(
        after.node(a).unwrap().attrs().get("extension"),
        document.node(a).unwrap().attrs().get("extension")
    );
    assert_eq!(
        after.node(a).unwrap().attrs().get("colwidth"),
        Some(&AttrValue::List(vec![
            AttrValue::Integer(100),
            AttrValue::Integer(0)
        ]))
    );
    for id in [empty, mixed, atom, code, quote, quote_text] {
        assert_eq!(after.node(id), document.node(id));
    }
    for id in [b, c, d] {
        assert!(after.node(id).is_none());
    }
    let point = InlinePoint::new(mixed, TextOffset::ZERO, 1, CursorAffinity::After);
    assert_eq!(
        applied.changes().map_inline_point(point, MapBias::End),
        MappedPosition::Mapped(point)
    );
    assert_eq!(
        applied.changes().map_node_selection(NodeSelection::new(b)),
        MappedPosition::Mapped(NodeSelection::new(a))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_selection(NodeSelection::new(atom)),
        MappedPosition::Mapped(NodeSelection::new(atom))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_gap(NodeGap::new(c, 0), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(a, 2))
    );
    let undone = applied.inverse().apply_with_changes(after).unwrap();
    assert_eq!(
        undone.changes().map_inline_point(point, MapBias::End),
        MappedPosition::Mapped(point)
    );
    assert_eq!(
        undone
            .changes()
            .map_node_gap(NodeGap::new(a, 2), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(c, 0))
    );
    round_trip(&document, &applied);
}

#[test]
fn split_preserves_survivor_content_and_allocates_header_units_with_sliced_widths() {
    let mut builder = NodeStoreBuilder::new();
    let text = paragraph(&mut builder, "retained");
    let attributes = attrs(&[
        ("rowspan", AttrValue::Integer(2)),
        ("colspan", AttrValue::Integer(3)),
        (
            "colwidth",
            AttrValue::List(vec![
                AttrValue::Integer(100),
                AttrValue::Integer(0),
                AttrValue::Integer(120),
            ]),
        ),
        ("background", AttrValue::String("red".into())),
        ("custom-null", AttrValue::Null),
    ]);
    let a = cell(&mut builder, NodeKind::TableHeader, attributes, &[text]);
    let r0 = row(&mut builder, &[a]);
    let r1 = row(&mut builder, &[]);
    let (document, table) = finish(builder, &[r0, r1]);
    let applied = apply(
        &document,
        TransactionStep::SplitTableCell { table, cell: a },
    );
    let after = applied.document();
    let grid = after.table_grid(table).unwrap();
    assert!(!grid.has_spans());
    assert_eq!(grid.origins().len(), 6);
    assert_eq!(grid.slot(0, 0), Some(a));
    assert_eq!(children(after, a), vec![text]);
    assert_eq!(after.node(text), document.node(text));
    assert_eq!(applied.changes().steps().len(), 5);
    for origin in grid.origins() {
        let node = after.node(origin.cell()).unwrap();
        assert_eq!(node.kind(), &NodeKind::TableHeader);
        assert_eq!(node.attrs().get("rowspan"), Some(&AttrValue::Integer(1)));
        assert_eq!(node.attrs().get("colspan"), Some(&AttrValue::Integer(1)));
        assert_eq!(node.attrs().get("custom-null"), Some(&AttrValue::Null));
        assert_eq!(
            node.attrs().get("background"),
            Some(&AttrValue::String("red".into()))
        );
        let width = [100, 0, 120][origin.column()];
        assert_eq!(
            node.attrs().get("colwidth"),
            Some(&AttrValue::List(vec![AttrValue::Integer(width)]))
        );
        if origin.cell() != a {
            assert!(document.node(origin.cell()).is_none());
            let blocks = children(after, origin.cell());
            assert_eq!(blocks.len(), 1);
            assert!(document.node(blocks[0]).is_none());
            assert_eq!(
                after.node(blocks[0]).unwrap().content(),
                &NodeContent::empty_inline()
            );
        }
    }
    assert_eq!(
        applied
            .changes()
            .map_node_gap(NodeGap::new(r1, 0), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(r1, 3))
    );
    round_trip(&document, &applied);
}

#[test]
fn merge_respects_existing_spans_and_rejects_partially_intersecting_rectangle() {
    let mut builder = NodeStoreBuilder::new();
    let pa = paragraph(&mut builder, "a");
    let pb = paragraph(&mut builder, "b");
    let pc = paragraph(&mut builder, "c");
    let a = cell(
        &mut builder,
        NodeKind::TableCell,
        attrs(&[("rowspan", AttrValue::Integer(2))]),
        &[pa],
    );
    let b = cell(&mut builder, NodeKind::TableCell, NodeAttrs::empty(), &[pb]);
    let c = cell(&mut builder, NodeKind::TableCell, NodeAttrs::empty(), &[pc]);
    let r0 = row(&mut builder, &[a, b]);
    let r1 = row(&mut builder, &[c]);
    let (document, table) = finish(builder, &[r0, r1]);
    let before = document.clone();
    for rect in [
        TableRect::new(1, 0, 2, 2).unwrap(),
        TableRect::new(0, 0, 3, 2).unwrap(),
    ] {
        let result = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::MergeTableCells { table, rect })
            .apply_with_changes(&document);
        assert_eq!(result.unwrap_err(), Error::InvalidSelection);
        same_snapshot(&document, &before);
    }
    let merged = apply(
        &document,
        TransactionStep::MergeTableCells {
            table,
            rect: TableRect::new(0, 0, 2, 2).unwrap(),
        },
    );
    assert_eq!(children(merged.document(), a), vec![pa, pb, pc]);
    round_trip(&document, &merged);
}

#[test]
fn split_interior_span_preserves_other_cells_and_physical_row_order() {
    let mut builder = NodeStoreBuilder::new();
    let mut cells = Vec::new();
    for label in ["left", "span", "right", "lower-left", "lower-right"] {
        let p = paragraph(&mut builder, label);
        let attributes = if label == "span" {
            attrs(&[
                ("rowspan", AttrValue::Integer(2)),
                ("colspan", AttrValue::Integer(2)),
            ])
        } else {
            NodeAttrs::empty()
        };
        cells.push(cell(&mut builder, NodeKind::TableCell, attributes, &[p]));
    }
    let r0 = row(&mut builder, &cells[..3]);
    let r1 = row(&mut builder, &cells[3..]);
    let (document, table) = finish(builder, &[r0, r1]);
    let split = apply(
        &document,
        TransactionStep::SplitTableCell {
            table,
            cell: cells[1],
        },
    );
    let after = split.document();
    let grid = after.table_grid(table).unwrap();
    assert_eq!(grid.slot(0, 0), Some(cells[0]));
    assert_eq!(grid.slot(0, 1), Some(cells[1]));
    assert_eq!(grid.slot(0, 3), Some(cells[2]));
    assert_eq!(grid.slot(1, 0), Some(cells[3]));
    assert_eq!(grid.slot(1, 3), Some(cells[4]));
    for id in [cells[0], cells[2], cells[3], cells[4]] {
        assert_eq!(after.node(id), document.node(id));
    }
    round_trip(&document, &split);
}

#[test]
fn merge_and_split_preserve_missing_and_null_width_presence() {
    for width in [None, Some(AttrValue::Null)] {
        let mut builder = NodeStoreBuilder::new();
        let p = paragraph(&mut builder, "");
        let q = paragraph(&mut builder, "");
        let attributes = width
            .clone()
            .map_or_else(NodeAttrs::empty, |width| attrs(&[("colwidth", width)]));
        let a = cell(&mut builder, NodeKind::TableCell, attributes, &[p]);
        let b = cell(&mut builder, NodeKind::TableCell, NodeAttrs::empty(), &[q]);
        let r = row(&mut builder, &[a, b]);
        let (document, table) = finish(builder, &[r]);
        let merged = apply(
            &document,
            TransactionStep::MergeTableCells {
                table,
                rect: TableRect::new(0, 0, 1, 2).unwrap(),
            },
        );
        assert_eq!(
            children(merged.document(), a),
            vec![p, q],
            "Core does not filter empty blocks"
        );
        assert_eq!(
            merged.document().node(a).unwrap().attrs().get("rowspan"),
            None
        );
        let split = apply(
            merged.document(),
            TransactionStep::SplitTableCell { table, cell: a },
        );
        for origin in split.document().table_grid(table).unwrap().origins() {
            assert_eq!(
                split
                    .document()
                    .node(origin.cell())
                    .unwrap()
                    .attrs()
                    .get("colwidth"),
                width.as_ref()
            );
            assert_eq!(
                split
                    .document()
                    .node(origin.cell())
                    .unwrap()
                    .attrs()
                    .get("rowspan"),
                None
            );
        }
        round_trip(&document, &merged);
        round_trip(merged.document(), &split);
    }
}

#[test]
fn stale_inverse_fails_without_clobbering_a_changed_survivor_or_consuming_ids() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "a");
    let q = paragraph(&mut builder, "b");
    let a = cell(&mut builder, NodeKind::TableCell, NodeAttrs::empty(), &[p]);
    let b = cell(&mut builder, NodeKind::TableCell, NodeAttrs::empty(), &[q]);
    let r = row(&mut builder, &[a, b]);
    let (document, table) = finish(builder, &[r]);
    let merged = apply(
        &document,
        TransactionStep::MergeTableCells {
            table,
            rect: TableRect::new(0, 0, 1, 2).unwrap(),
        },
    );
    assert!(
        merged.inverse().apply_with_changes(&document).is_err(),
        "live restoration IDs cannot be overwritten"
    );
    let changed = apply(
        merged.document(),
        TransactionStep::SetNodeAttrs {
            node: a,
            attrs: attrs(&[
                ("colspan", AttrValue::Integer(2)),
                ("new", AttrValue::Bool(true)),
            ]),
        },
    );
    let before = changed.document().clone();
    assert_eq!(
        merged.inverse().apply_with_changes(&before).unwrap_err(),
        Error::InvalidTransaction
    );
    same_snapshot(changed.document(), &before);
    let split = apply(&before, TransactionStep::SplitTableCell { table, cell: a });
    let direct = apply(
        changed.document(),
        TransactionStep::SplitTableCell { table, cell: a },
    );
    same_snapshot(split.document(), direct.document());
}

#[test]
fn late_failure_rolls_back_all_allocations_and_original_snapshot() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "a");
    let a = cell(
        &mut builder,
        NodeKind::TableCell,
        attrs(&[("colspan", AttrValue::Integer(2))]),
        &[p],
    );
    let r = row(&mut builder, &[a]);
    let (document, table) = finish(builder, &[r]);
    let before = document.clone();
    let split = TransactionStep::SplitTableCell { table, cell: a };
    let result = Transaction::new(TransactionOrigin::UserInput)
        .with_step(split.clone())
        .with_step(TransactionStep::RemoveNode {
            node: document.root(),
        })
        .apply_with_changes(&document);
    assert_eq!(result.unwrap_err(), Error::InvalidTransaction);
    same_snapshot(&document, &before);
    same_snapshot(
        apply(&document, split.clone()).document(),
        apply(&before, split).document(),
    );
}

#[test]
fn split_resource_limit_rejects_before_identity_allocation() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "large");
    let a = cell(
        &mut builder,
        NodeKind::TableCell,
        attrs(&[("colspan", AttrValue::Integer(100_001))]),
        &[p],
    );
    let r = row(&mut builder, &[a]);
    let (document, table) = finish(builder, &[r]);
    let before = document.clone();
    let result = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SplitTableCell { table, cell: a })
        .apply_with_changes(&document);
    assert_eq!(result.unwrap_err(), Error::TableResourceLimit);
    same_snapshot(&document, &before);
}

#[test]
fn unit_split_single_origin_merge_and_foreign_target_are_rejected() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "a");
    let a = cell(&mut builder, NodeKind::TableCell, NodeAttrs::empty(), &[p]);
    let r = row(&mut builder, &[a]);
    let (document, table) = finish(builder, &[r]);
    for step in [
        TransactionStep::SplitTableCell { table, cell: a },
        TransactionStep::MergeTableCells {
            table,
            rect: TableRect::new(0, 0, 1, 1).unwrap(),
        },
        TransactionStep::SplitTableCell { table, cell: p },
        TransactionStep::MergeTableCells {
            table: p,
            rect: TableRect::new(0, 0, 1, 1).unwrap(),
        },
    ] {
        assert!(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(step)
                .apply_with_changes(&document)
                .is_err()
        );
    }
}

#[test]
fn split_inverse_deletes_only_fresh_cell_subtrees_and_redo_reuses_exact_ids() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "a");
    let a = cell(
        &mut builder,
        NodeKind::TableCell,
        attrs(&[("colspan", AttrValue::Integer(2))]),
        &[p],
    );
    let r = row(&mut builder, &[a]);
    let (document, table) = finish(builder, &[r]);
    let split = apply(
        &document,
        TransactionStep::SplitTableCell { table, cell: a },
    );
    let created = match split.changes().steps()[0] {
        StepMap::NodeInserted { inserted, .. } => inserted,
        ref other => panic!("unexpected map {other:?}"),
    };
    let new_p = children(split.document(), created)[0];
    let undo = split
        .inverse()
        .apply_with_changes(split.document())
        .unwrap();
    assert_eq!(
        undo.changes().map_node_selection(NodeSelection::new(new_p)),
        MappedPosition::Deleted
    );
    assert_eq!(
        undo.changes().map_node_selection(NodeSelection::new(p)),
        MappedPosition::Mapped(NodeSelection::new(p))
    );
    round_trip(&document, &split);
}

#[test]
fn inverse_rejects_affected_row_moved_to_another_valid_table() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "span");
    let a = cell(
        &mut builder,
        NodeKind::TableCell,
        attrs(&[("colspan", AttrValue::Integer(2))]),
        &[p],
    );
    let moved_row = row(&mut builder, &[a]);
    let mut other_rows = Vec::new();
    for _ in 0..2 {
        let mut cells = Vec::new();
        for _ in 0..2 {
            let p = paragraph(&mut builder, "other");
            cells.push(cell(
                &mut builder,
                NodeKind::TableCell,
                NodeAttrs::empty(),
                &[p],
            ));
        }
        other_rows.push(row(&mut builder, &cells));
    }
    let source = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([moved_row, other_rows[0]]),
        )
        .unwrap();
    let target = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([other_rows[1]]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([source, target]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let split = apply(
        &document,
        TransactionStep::SplitTableCell {
            table: source,
            cell: a,
        },
    );
    let mut nodes = vec![split.document().node(moved_row).unwrap().clone()];
    for cell in children(split.document(), moved_row) {
        nodes.push(split.document().node(cell).unwrap().clone());
        nodes.extend(
            children(split.document(), cell)
                .into_iter()
                .map(|id| split.document().node(id).unwrap().clone()),
        );
    }
    let moved = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: moved_row })
        .with_step(TransactionStep::RestoreSubtree {
            parent: target,
            index: 1,
            root: moved_row,
            nodes,
        })
        .apply_with_changes(split.document())
        .unwrap();
    assert_eq!(moved.document().parent_of(moved_row), Some(target));
    moved.document().table_grid(source).unwrap();
    moved.document().table_grid(target).unwrap();
    let before = moved.document().clone();
    assert_eq!(
        split
            .inverse()
            .apply_with_changes(moved.document())
            .unwrap_err(),
        Error::InvalidTransaction
    );
    same_snapshot(moved.document(), &before);
    round_trip(&document, &split);
}

#[test]
fn inverse_rejects_affected_row_or_ancestor_moved_into_a_nested_table() {
    for vertical in [false, true] {
        let mut builder = NodeStoreBuilder::new();
        let p = paragraph(&mut builder, "span");
        let attributes = if vertical {
            attrs(&[("rowspan", AttrValue::Integer(2))])
        } else {
            attrs(&[("colspan", AttrValue::Integer(2))])
        };
        let a = cell(&mut builder, NodeKind::TableCell, attributes, &[p]);
        let mut top_cells = vec![a];
        if vertical {
            let p = paragraph(&mut builder, "top-right");
            top_cells.push(cell(
                &mut builder,
                NodeKind::TableCell,
                NodeAttrs::empty(),
                &[p],
            ));
        }
        let moved_row = row(&mut builder, &top_cells);
        let mut nested_cells = Vec::new();
        for _ in 0..2 {
            let p = paragraph(&mut builder, "nested");
            nested_cells.push(cell(
                &mut builder,
                NodeKind::TableCell,
                NodeAttrs::empty(),
                &[p],
            ));
        }
        let nested_row = row(&mut builder, &nested_cells);
        let nested = builder
            .insert(
                NodeKind::Table,
                NodeAttrs::empty(),
                NodeContent::children([nested_row]),
            )
            .unwrap();
        let holder = cell(
            &mut builder,
            NodeKind::TableCell,
            NodeAttrs::empty(),
            &[nested],
        );
        let mut lower_cells = vec![holder];
        if !vertical {
            let p = paragraph(&mut builder, "lower-right");
            lower_cells.push(cell(
                &mut builder,
                NodeKind::TableCell,
                NodeAttrs::empty(),
                &[p],
            ));
        }
        let lower = row(&mut builder, &lower_cells);
        let (document, table) = finish(builder, &[moved_row, lower]);
        let split = apply(
            &document,
            TransactionStep::SplitTableCell { table, cell: a },
        );
        // In the vertical case, the top row itself was not rewritten by split:
        // the affected cell's ancestor edge must still bind it to this table.
        let mut nodes = vec![split.document().node(moved_row).unwrap().clone()];
        for cell in children(split.document(), moved_row) {
            nodes.push(split.document().node(cell).unwrap().clone());
            nodes.extend(
                children(split.document(), cell)
                    .into_iter()
                    .map(|id| split.document().node(id).unwrap().clone()),
            );
        }
        let moved = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::RemoveNode { node: moved_row })
            .with_step(TransactionStep::RestoreSubtree {
                parent: nested,
                index: 1,
                root: moved_row,
                nodes,
            })
            .apply_with_changes(split.document())
            .unwrap();
        assert_eq!(
            moved.document().node(moved_row),
            split.document().node(moved_row)
        );
        assert_eq!(moved.document().parent_of(moved_row), Some(nested));
        moved.document().table_grid(table).unwrap();
        moved.document().table_grid(nested).unwrap();
        assert_eq!(
            split
                .inverse()
                .apply_with_changes(moved.document())
                .unwrap_err(),
            Error::InvalidTransaction
        );
        round_trip(&document, &split);
    }
}
