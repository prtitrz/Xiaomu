use super::*;
use crate::document::AttrValue;

fn assert_rejected(context: &mut ApplyContext, restore: &TableCellRestore) {
    let before = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context.apply_restore_table_cells(restore).unwrap_err(),
        Error::InvalidTransaction
    );
    assert_eq!(context.store, before);
    assert_eq!(context.next_node_id, ceiling);
}

#[test]
fn stale_inverse_rejects_changed_descendant_row_and_reordered_rows() {
    let (source, source_table) = rich_grid(2, 2, 1);
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let (target, table) = unit_grid(4, 4);
    let result = apply(&target, table, TableRect::new(1, 1, 3, 3).unwrap(), &tree);
    let TransactionStep::RestoreTableCells { restore } = &result.inverse().steps()[0] else {
        panic!("inverse")
    };
    let grid = result.document().table_grid(table).unwrap();
    for row in [false, true] {
        let mut context = context(result.document());
        let node = if row {
            grid.row_id(2).unwrap()
        } else {
            context.children(grid.slot(1, 1).unwrap()).unwrap()[0]
        };
        let content = context.content_of(node).unwrap();
        context
            .rewrite_node(node, attrs(&[("changed", AttrValue::Null)]), content)
            .unwrap();
        assert_rejected(&mut context, restore);
    }
    // Reorder unaffected rows so the grid stays valid; the table guard still
    // binds the exact row list, beyond just the two changed row payloads.
    let mut context = context(result.document());
    let mut rows = context.children(table).unwrap();
    rows.swap(0, 3);
    let table_attrs = context.store.get(table).unwrap().attrs().clone();
    context
        .rewrite_node(table, table_attrs, NodeContent::children(rows))
        .unwrap();
    assert_rejected(&mut context, restore);
}

#[test]
fn stale_inverse_rejects_reparented_descendant_and_occupied_restore_identity() {
    let (source, source_table) = rich_grid(2, 2, 1);
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let (target, table) = unit_grid(4, 4);
    let rect = TableRect::new(1, 1, 3, 3).unwrap();
    let result = apply(&target, table, rect, &tree);
    let TransactionStep::RestoreTableCells { restore } = &result.inverse().steps()[0] else {
        panic!("inverse")
    };
    let grid = result.document().table_grid(table).unwrap();
    let mut moved = context(result.document());
    let cell = grid.slot(1, 1).unwrap();
    let outside = grid.slot(0, 0).unwrap();
    let mut children = moved.children(cell).unwrap();
    let child = children.remove(0);
    let mut outside_children = moved.children(outside).unwrap();
    outside_children.push(child);
    for (id, children) in [(cell, children), (outside, outside_children)] {
        let old = moved.store.get(id).unwrap();
        let replacement = old.with_content(NodeContent::children(children)).unwrap();
        moved.store.replace_node_mut(replacement).unwrap();
    }
    let actual =
        crate::transaction::table_restore::expected_parents(&moved.store, table, &restore.expected)
            .unwrap();
    assert_ne!(actual, restore.expected_parents);
    assert_rejected(&mut moved, restore);
    let mut occupied = context(result.document());
    let old_cell = target.table_grid(table).unwrap().slot(1, 1).unwrap();
    let old_paragraph = target
        .node(old_cell)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    occupied
        .store
        .insert_node_mut(target.node(old_paragraph).unwrap().clone())
        .unwrap();
    assert_rejected(&mut occupied, restore);
}

#[test]
fn stale_inverse_rejects_affected_row_moved_to_another_table() {
    let (target, table) = unit_grid(4, 4);
    let sibling_tree = TableTreeTemplate::capture(&target, table).unwrap();
    let target = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableTree {
            parent: target.root(),
            index: 1,
            tree: sibling_tree,
        })
        .apply(&target)
        .unwrap();
    let sibling = target
        .node(target.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[1];
    let (source, source_table) = unit_grid(2, 2);
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let result = apply(&target, table, TableRect::new(1, 1, 3, 3).unwrap(), &tree);
    let TransactionStep::RestoreTableCells { restore } = &result.inverse().steps()[0] else {
        panic!("inverse")
    };
    let mut moved = context(result.document());
    let mut target_rows = moved.children(table).unwrap();
    let mut sibling_rows = moved.children(sibling).unwrap();
    std::mem::swap(&mut target_rows[1], &mut sibling_rows[1]);
    for (id, rows) in [(table, target_rows), (sibling, sibling_rows)] {
        let replacement = moved
            .store
            .get(id)
            .unwrap()
            .with_content(NodeContent::children(rows))
            .unwrap();
        moved.store.replace_node_mut(replacement).unwrap();
    }
    TableGrid::from_store(&moved.store, sibling, &mut TableGridBudget::default()).unwrap();
    assert_rejected(&mut moved, restore);
}

#[test]
fn rejected_bounds_dimensions_and_nonclosed_rect_do_not_change_store_or_allocator() {
    assert_eq!(
        TableRect::new(1, 1, 1, 2).unwrap_err(),
        Error::InvalidSelection
    );
    assert_eq!(
        TableRect::new(2, 0, 1, 1).unwrap_err(),
        Error::InvalidSelection
    );
    let (source, source_table) = unit_grid(2, 2);
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let (target, table) = rich_grid(2, 2, 1);
    for rect in [
        TableRect::new(0, 0, 3, 2).unwrap(),
        TableRect::new(0, 0, 1, 1).unwrap(),
    ] {
        let mut context = context(&target);
        let before = context.store.clone();
        let ceiling = context.next_node_id;
        assert_eq!(
            context
                .apply_replace_table_rect(table, rect, &tree)
                .unwrap_err(),
            Error::InvalidSelection
        );
        assert_eq!(context.store, before);
        assert_eq!(context.next_node_id, ceiling);
    }
    let (target, table) = unit_grid(4, 4);
    let mut context = context(&target);
    let before = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context
            .apply_replace_table_rect(table, TableRect::new(0, 0, 4, 4).unwrap(), &tree)
            .unwrap_err(),
        Error::InvalidSelection
    );
    assert_eq!(context.store, before);
    assert_eq!(context.next_node_id, ceiling);
}

#[test]
fn final_budget_subtracts_removed_nested_grids_and_omits_source_outer_grid() {
    // Keeping the old nested grid or counting the source outer grid again
    // would exceed the slot limit in these otherwise valid replacements.
    for (columns, nested, other) in [(1, 600_000, 399_999), (600_000, 0, 400_000)] {
        let (target, table) = budget_grid(columns, nested, other);
        let (source, source_table) = budget_grid(columns, nested, 0);
        let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
        let result = apply(
            &target,
            table,
            TableRect::new(0, 0, 1, columns).unwrap(),
            &tree,
        );
        round_trip(&target, &result);
    }
}

#[test]
fn final_budget_counts_surviving_and_incoming_nested_grids_before_mutation() {
    let (target, table) = budget_grid(1, 500_000, 400_000);
    let (source, source_table) = budget_grid(1, 600_000, 0);
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let mut context = context(&target);
    let before = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context
            .apply_replace_table_rect(table, TableRect::new(0, 0, 1, 1).unwrap(), &tree)
            .unwrap_err(),
        Error::TableResourceLimit
    );
    assert_eq!(context.store, before);
    assert_eq!(context.next_node_id, ceiling);
}

#[test]
fn complete_identity_range_is_checked_and_discarded_wrappers_consume_none() {
    let (target, table) = unit_grid(1, 1);
    let tree = TableTreeTemplate::capture(&target, table).unwrap();
    assert_eq!(tree.node_count(), 4);
    let mut context = context(&target);
    context.next_node_id = u64::MAX - 1;
    let before = context.store.clone();
    let rect = TableRect::new(0, 0, 1, 1).unwrap();
    assert_eq!(
        context
            .apply_replace_table_rect(table, rect, &tree)
            .unwrap_err(),
        Error::NodeIdExhausted
    );
    assert_eq!(context.store, before);
    assert_eq!(context.next_node_id, u64::MAX - 1);
    context.next_node_id = u64::MAX - 2;
    context
        .apply_replace_table_rect(table, rect, &tree)
        .unwrap();
    assert_eq!(context.next_node_id, u64::MAX);
}

#[test]
fn invalid_local_materialization_and_late_transaction_failure_consume_no_ids() {
    let (target, table) = unit_grid(2, 2);
    let tree = TableTreeTemplate::capture(&target, table).unwrap();
    let rect = TableRect::new(0, 0, 2, 2).unwrap();
    let baseline = apply(&target, table, rect, &tree);
    let mut broken = TableTreeTemplate::capture(&target, table).unwrap();
    let data = std::sync::Arc::get_mut(&mut broken.data).unwrap();
    let cell = data
        .nodes
        .iter_mut()
        .find(|node| node.kind.is_table_cell())
        .unwrap();
    cell.content = TemplateContent::Children(vec![0]);
    let mut context = context(&target);
    let before = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context
            .apply_replace_table_rect(table, rect, &broken)
            .unwrap_err(),
        Error::InvalidTransaction
    );
    assert_eq!(context.store, before);
    assert_eq!(context.next_node_id, ceiling);
    let failed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceTableRect {
            table,
            rect,
            tree: tree.clone(),
        })
        .with_step(TransactionStep::RemoveNode {
            node: target.root(),
        })
        .apply_with_changes(&target);
    assert_eq!(failed.unwrap_err(), Error::InvalidTransaction);
    assert_eq!(target.store(), &before);
    assert_eq!(target.next_node_id(), ceiling);
    let repeated = apply(&target, table, rect, &tree);
    assert_eq!(repeated.document().store(), baseline.document().store());
    assert_eq!(
        repeated.document().next_node_id(),
        baseline.document().next_node_id()
    );
}
