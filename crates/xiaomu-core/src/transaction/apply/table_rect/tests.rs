use super::*;
use crate::document::XiaomuDocument;
use crate::mapping::{MapBias, MappedPosition};
use crate::selection::{CursorAffinity, InlinePoint, NodeGap, NodeSelection, TextPoint};
use crate::text::TextOffset;
use crate::transaction::{AppliedTransaction, Transaction, TransactionOrigin};

mod failures;
mod fixtures;
use fixtures::*;

#[test]
fn nonzero_rect_copies_rich_forest_and_keeps_target_wrappers_and_outside_payloads() {
    let (source, source_table) = rich_grid(2, 2, 1);
    let source_before = source.store().clone();
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let template_before = tree.clone();
    let (target, table) = unit_grid(4, 4);
    let grid = target.table_grid(table).unwrap();
    let rect = TableRect::new(1, 1, 3, 3).unwrap();
    let removed: BTreeSet<_> = grid
        .unique_cells_in(rect)
        .unwrap()
        .into_iter()
        .flat_map(|cell| context(&target).collect_subtree(cell))
        .collect();
    let applied = apply(&target, table, rect, &tree);
    let after = applied.document();
    let new_grid = after.table_grid(table).unwrap();
    let source_cell = source.table_grid(source_table).unwrap().slot(0, 0).unwrap();
    let new_cell = new_grid.slot(1, 1).unwrap();
    let mut pairs = BTreeMap::new();
    compare_copy(&source, source_cell, after, new_cell, &mut pairs);
    assert_eq!(pairs.len(), tree.node_count() - 1 - 2);
    let new_ids: BTreeSet<_> = pairs.values().copied().collect();
    let expected_ids: BTreeSet<_> = (target.next_node_id()
        ..target.next_node_id() + pairs.len() as u64)
        .map(NodeId::from_allocated)
        .collect();
    assert_eq!(new_ids, expected_ids, "only cells/descendants consume IDs");
    assert_eq!(
        after.next_node_id(),
        target.next_node_id() + pairs.len() as u64
    );
    assert!(new_ids.iter().all(|id| target.node(*id).is_none()));
    assert_eq!(
        (
            new_grid.rows(),
            new_grid.columns(),
            new_grid.origins().len()
        ),
        (4, 4, 13)
    );
    assert_eq!(after.node(table), target.node(table));
    for row in 0..4 {
        let id = grid.row_id(row).unwrap();
        assert_eq!(new_grid.row_id(row), Some(id));
        assert_eq!(
            after.node(id).unwrap().attrs(),
            target.node(id).unwrap().attrs()
        );
    }
    let affected_rows = [grid.row_id(1).unwrap(), grid.row_id(2).unwrap()];
    for node in target.store().iter() {
        if removed.contains(&node.id()) {
            assert!(after.node(node.id()).is_none());
        } else if !affected_rows.contains(&node.id()) {
            assert_eq!(after.node(node.id()), Some(node));
            if node.id() != table {
                assert!(target.store().shares_node_payload(after.store(), node.id()));
            }
        }
    }
    assert_eq!(source.store(), &source_before);
    assert_eq!(tree, template_before);
    round_trip(&target, &applied);
}

#[test]
fn different_physical_counts_and_covered_empty_rows_are_exactly_reversible() {
    let (rich, rich_table) = rich_grid(2, 2, 1);
    let (unit, unit_table) = unit_grid(2, 2);
    let rect = TableRect::new(0, 0, 2, 2).unwrap();
    for (source, source_table, target, table, count) in [
        (&rich, rich_table, &unit, unit_table, 1),
        (&unit, unit_table, &rich, rich_table, 4),
    ] {
        let tree = TableTreeTemplate::capture(source, source_table).unwrap();
        let result = apply(target, table, rect, &tree);
        let grid = result.document().table_grid(table).unwrap();
        assert_eq!(grid.origins().len(), count);
        let row = result.document().node(grid.row_id(1).unwrap()).unwrap();
        assert_eq!(
            row.content().as_children().unwrap().len(),
            if count == 1 { 0 } else { 2 }
        );
        round_trip(target, &result);
    }
}

#[test]
fn maps_delete_complete_old_and_new_subtrees_and_shift_outside_row_gaps() {
    let (source, source_table) = rich_grid(2, 2, 1);
    let (target, table) = unit_grid(4, 4);
    let grid = target.table_grid(table).unwrap();
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let applied = apply(&target, table, TableRect::new(1, 1, 3, 3).unwrap(), &tree);
    let mut working_rows: BTreeMap<_, _> = (0..4)
        .map(|row| {
            let row = grid.row_id(row).unwrap();
            (
                row,
                target
                    .node(row)
                    .unwrap()
                    .content()
                    .as_children()
                    .unwrap()
                    .to_vec(),
            )
        })
        .collect();
    for step in applied.changes().steps() {
        match step {
            StepMap::NodeRemoved {
                parent,
                index,
                removed,
            } => {
                let cell = working_rows.get_mut(parent).unwrap().remove(*index);
                assert_eq!(removed, &context(&target).collect_subtree(cell));
                for id in removed {
                    assert_eq!(
                        applied
                            .changes()
                            .map_node_selection(NodeSelection::new(*id)),
                        MappedPosition::Deleted
                    );
                    assert_eq!(
                        applied
                            .changes()
                            .map_node_gap(NodeGap::new(*id, 0), MapBias::Start),
                        MappedPosition::Deleted
                    );
                    let point = InlinePoint::new(*id, TextOffset::ZERO, 0, CursorAffinity::After);
                    assert_eq!(
                        applied.changes().map_inline_point(point, MapBias::End),
                        MappedPosition::Deleted
                    );
                    let point = TextPoint::new(*id, TextOffset::ZERO, CursorAffinity::After);
                    assert_eq!(
                        applied.changes().map_text_point(point, MapBias::End),
                        MappedPosition::Deleted
                    );
                }
            }
            StepMap::NodeInserted {
                parent,
                index,
                inserted,
            } => working_rows
                .get_mut(parent)
                .unwrap()
                .insert(*index, *inserted),
            other => panic!("unexpected map: {other:?}"),
        }
    }
    for (row, children) in working_rows {
        assert_eq!(
            applied
                .document()
                .node(row)
                .unwrap()
                .content()
                .as_children()
                .unwrap(),
            children
        );
    }
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    for (row_index, end) in [(1, 3), (2, 2)] {
        let row = grid.row_id(row_index).unwrap();
        let old = NodeGap::new(row, 4);
        let new = NodeGap::new(row, end);
        for bias in [MapBias::Start, MapBias::End] {
            assert_eq!(
                applied.changes().map_node_gap(old, bias),
                MappedPosition::Mapped(new)
            );
            assert_eq!(
                undo.changes().map_node_gap(new, bias),
                MappedPosition::Mapped(old)
            );
            let before = NodeGap::new(row, 0);
            assert_eq!(
                applied.changes().map_node_gap(before, bias),
                MappedPosition::Mapped(before)
            );
        }
    }
    for node in applied
        .document()
        .store()
        .iter()
        .filter(|node| target.node(node.id()).is_none())
    {
        assert_eq!(
            undo.changes()
                .map_node_selection(NodeSelection::new(node.id())),
            MappedPosition::Deleted
        );
    }
    let outside = grid.slot(0, 0).unwrap();
    assert_eq!(
        applied
            .changes()
            .map_node_selection(NodeSelection::new(outside)),
        MappedPosition::Mapped(NodeSelection::new(outside))
    );
    let gap = NodeGap::new(table, 2);
    assert_eq!(
        applied.changes().map_node_gap(gap, MapBias::End),
        MappedPosition::Mapped(gap)
    );
}

#[test]
fn replacement_inserts_into_a_covered_row_before_its_surviving_right_origin() {
    let (source, source_table) = rich_grid(2, 2, 1);
    let (target, table) = unit_grid(4, 4);
    let rect = TableRect::new(1, 1, 3, 3).unwrap();
    let first = apply(
        &target,
        table,
        rect,
        &TableTreeTemplate::capture(&source, source_table).unwrap(),
    );
    let before = first.document();
    let grid = before.table_grid(table).unwrap();
    let covered_row = grid.row_id(2).unwrap();
    let right = grid.slot(2, 3).unwrap();
    assert_eq!(grid.placement(right).unwrap().physical_index(), 1);
    let (unit, unit_table) = unit_grid(2, 2);
    let tree = TableTreeTemplate::capture(&unit, unit_table).unwrap();
    let result = apply(before, table, rect, &tree);
    let new_grid = result.document().table_grid(table).unwrap();
    assert_eq!(new_grid.slot(2, 3), Some(right));
    assert_eq!(new_grid.placement(right).unwrap().physical_index(), 3);
    assert_eq!(
        result
            .changes()
            .map_node_gap(NodeGap::new(covered_row, 2), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(covered_row, 4))
    );
    let [StepMap::NodeRemoved { removed, .. }, ..] = result.changes().steps() else {
        panic!("rich removal")
    };
    let old_cell = grid.slot(1, 1).unwrap();
    assert_eq!(removed, &context(before).collect_subtree(old_cell));
    let atoms: Vec<_> = removed
        .iter()
        .filter(|id| matches!(before.node(**id).unwrap().kind(), NodeKind::InlineAtom(_)))
        .collect();
    assert_eq!(
        atoms.len(),
        4,
        "both outer and nested hard breaks/mentions are removed"
    );
    for atom in atoms {
        assert_eq!(
            result
                .changes()
                .map_node_selection(NodeSelection::new(*atom)),
            MappedPosition::Deleted
        );
    }
    round_trip(before, &result);
}

#[test]
fn inverse_permits_unrelated_outside_descendant_edits() {
    let (source, source_table) = rich_grid(2, 2, 1);
    let (target, table) = unit_grid(4, 4);
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let result = apply(&target, table, TableRect::new(1, 1, 3, 3).unwrap(), &tree);
    let outside_cell = target.table_grid(table).unwrap().slot(0, 0).unwrap();
    let outside = target
        .node(outside_cell)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let edit =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeAttrs {
            node: outside,
            attrs: attrs(&[("later", crate::document::AttrValue::Null)]),
        });
    let edited = edit.apply(result.document()).unwrap();
    let restored = result.inverse().apply(&edited).unwrap();
    assert_eq!(restored.store(), edit.apply(&target).unwrap().store());
}
