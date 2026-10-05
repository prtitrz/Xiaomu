use super::*;

fn rejected(ctx: &mut ApplyContext, table: NodeId, rect: TableRect, error: Error) {
    let store = ctx.store.clone();
    let next = ctx.next_node_id;
    assert_eq!(
        ctx.apply_isolate_table_rect(table, rect).unwrap_err(),
        error
    );
    assert_eq!(ctx.store, store);
    assert_eq!(ctx.store.map_storage_id(), store.map_storage_id());
    assert_eq!(ctx.next_node_id, next);
}

fn rejected_restore(ctx: &mut ApplyContext, restore: &TableCellRestore) {
    let store = ctx.store.clone();
    let next = ctx.next_node_id;
    assert_eq!(
        ctx.apply_restore_table_cells(restore).unwrap_err(),
        Error::InvalidTransaction
    );
    assert_eq!(ctx.store, store);
    assert_eq!(ctx.next_node_id, next);
}

#[test]
fn allocator_boundary_counts_all_pairs_and_failure_is_atomic() {
    let (before, table, _) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let rect = TableRect::new(1, 1, 2, 2).unwrap();
    let mut ctx = context(&before);
    ctx.next_node_id = u64::MAX - 7;
    rejected(&mut ctx, table, rect, Error::NodeIdExhausted);
    ctx.next_node_id = u64::MAX - 8;
    ctx.apply_isolate_table_rect(table, rect).unwrap();
    assert_eq!(ctx.next_node_id, u64::MAX);
    assert_eq!(ctx.store.len(), before.store().len() + 8);
}

#[test]
fn invalid_bounds_and_malformed_intermediate_grid_are_atomic() {
    let (before, table, cell) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    for rect in [
        TableRect::new(0, 0, 4, 3).unwrap(),
        TableRect::new(0, 0, 3, 4).unwrap(),
    ] {
        rejected(&mut context(&before), table, rect, Error::InvalidSelection);
    }
    let mut ctx = context(&before);
    let content = ctx.content_of(cell).unwrap();
    ctx.rewrite_node(
        cell,
        attrs(&[
            ("rowspan", AttrValue::Integer(4)),
            ("colspan", AttrValue::Integer(3)),
        ]),
        content,
    )
    .unwrap();
    rejected(
        &mut ctx,
        table,
        TableRect::new(1, 1, 2, 2).unwrap(),
        Error::InvalidTableStructure,
    );
}

#[test]
fn borrowed_budget_counts_cell_row_and_table_attrs_before_exchange() {
    let (before, table, cell) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let rect = TableRect::new(1, 1, 2, 2).unwrap();
    for id in [cell, table, children(&before, table)[1]] {
        let mut ctx = context(&before);
        let original = ctx.store.get(id).unwrap();
        let mut values: BTreeMap<_, _> = original
            .attrs()
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        values.insert(
            "large".into(),
            AttrValue::String("x".repeat(5 * 1024 * 1024)),
        );
        let content = original.content().clone();
        ctx.rewrite_node(id, NodeAttrs::new(values).unwrap(), content)
            .unwrap();
        rejected(&mut ctx, table, rect, Error::TableResourceLimit);
    }
}

#[test]
fn borrowed_budget_rejects_attr_depth_and_cumulative_values() {
    for value in [
        AttrValue::List(vec![AttrValue::Null; 40_000]),
        (0..64).fold(AttrValue::Null, |value, _| AttrValue::List(vec![value])),
    ] {
        let (before, table, _) = span_fixture(3, 3, NodeKind::TableCell, attrs(&[("deep", value)]));
        rejected(
            &mut context(&before),
            table,
            TableRect::new(1, 1, 2, 2).unwrap(),
            Error::TableResourceLimit,
        );
    }
}

#[test]
fn borrowed_preflight_counts_owned_rich_child_id_vectors() {
    let mut builder = NodeStoreBuilder::new();
    // The two source-cell payloads alone exceed one million accounted child
    // entries after eight passes, despite having only one physical cell.
    let blocks: Vec<_> = (0..62_501)
        .map(|_| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::empty_inline(),
                )
                .unwrap()
        })
        .collect();
    let cell = builder
        .insert(
            NodeKind::TableCell,
            attrs(&[("colspan", AttrValue::Integer(3))]),
            NodeContent::children(blocks),
        )
        .unwrap();
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
    let before = XiaomuDocument::new(root, builder.finish()).unwrap();
    rejected(
        &mut context(&before),
        table,
        TableRect::new(0, 1, 1, 2).unwrap(),
        Error::TableResourceLimit,
    );
}

#[test]
fn aggregate_grid_admission_keeps_nested_tables_in_the_budget() {
    let (before, table, original) =
        span_fixture(1, 600_000, NodeKind::TableCell, NodeAttrs::empty());
    let mut ctx = context(&before);
    let nested = *children(&before, original).last().unwrap();
    let nested_row = children(&before, nested)[0];
    let nested_cell = children(&before, nested_row)[0];
    let content = ctx.content_of(nested_cell).unwrap();
    ctx.rewrite_node(
        nested_cell,
        attrs(&[("colspan", AttrValue::Integer(400_001))]),
        content,
    )
    .unwrap();
    // Both grids are individually valid. Their intermediate aggregate is not.
    rejected(
        &mut ctx,
        table,
        TableRect::new(0, 1, 1, 2).unwrap(),
        Error::TableResourceLimit,
    );
}

#[test]
fn stale_inverse_guards_cells_rows_table_and_fresh_paragraphs() {
    let (before, table, cell) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let result = apply(&before, table, TableRect::new(1, 1, 2, 2).unwrap());
    let after = result.document();
    let new_cell = after.table_grid(table).unwrap().slot(1, 1).unwrap();
    let fresh_p = children(after, new_cell)[0];
    for id in [table, cell, children(after, table)[1], fresh_p] {
        let mut ctx = context(after);
        let old = ctx.store.get(id).unwrap();
        let mut values: BTreeMap<_, _> = old
            .attrs()
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        values.insert("changed".into(), AttrValue::Null);
        let content = old.content().clone();
        ctx.rewrite_node(id, NodeAttrs::new(values).unwrap(), content)
            .unwrap();
        rejected_restore(&mut ctx, expected_restore(&result));
    }
    let undo = result.inverse().apply_with_changes(after).unwrap();
    let mut occupied = context(undo.document());
    occupied
        .store
        .insert_node_mut(after.node(fresh_p).unwrap().clone())
        .unwrap();
    rejected_restore(&mut occupied, expected_restore(&undo));
}

#[test]
fn stale_inverse_rejects_affected_row_moved_under_nested_table() {
    let (before, table, cell) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let mut ctx = context(&before);
    let nested = *children(&before, cell).last().unwrap();
    let nested_row = children(&before, nested)[0];
    let nested_cell = children(&before, nested_row)[0];
    let content = ctx.content_of(nested_cell).unwrap();
    ctx.rewrite_node(
        nested_cell,
        attrs(&[("colspan", AttrValue::Integer(3))]),
        content,
    )
    .unwrap();
    let before = XiaomuDocument::new(ctx.root, ctx.store).unwrap();
    let result = apply(&before, table, TableRect::new(1, 1, 2, 2).unwrap());
    let mut moved = context(result.document());
    let mut rows = moved.children(table).unwrap();
    let middle_row = rows[1];
    rows[1] = nested_row;
    for (id, rows) in [(table, rows), (nested, vec![middle_row])] {
        let old = moved.store.get(id).unwrap();
        moved
            .store
            .replace_node_mut(old.with_content(NodeContent::children(rows)).unwrap())
            .unwrap();
    }
    TableGrid::from_store(&moved.store, table, &mut TableGridBudget::default()).unwrap();
    TableGrid::from_store(&moved.store, nested, &mut TableGridBudget::default()).unwrap();
    let restore = expected_restore(&result);
    let actual =
        crate::transaction::table_restore::expected_parents(&moved.store, table, &restore.expected)
            .unwrap();
    assert_ne!(actual, restore.expected_parents);
    rejected_restore(&mut moved, restore);
}

#[test]
fn preserved_rich_descendant_edit_is_not_overwritten_by_inverse() {
    let (before, table, cell) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let result = apply(&before, table, TableRect::new(1, 1, 2, 2).unwrap());
    let paragraph = children(&before, cell)[0];
    let changed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: paragraph,
            attrs: attrs(&[("changed", AttrValue::Null)]),
        })
        .apply(result.document())
        .unwrap();
    let undone = result.inverse().apply(&changed).unwrap();
    assert_eq!(undone.node(paragraph), changed.node(paragraph));
    assert_eq!(children(&undone, cell), children(&before, cell));
}

#[test]
fn later_failed_step_rolls_back_isolation_and_all_reserved_ids() {
    let (before, table, _) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let rect = TableRect::new(1, 1, 2, 2).unwrap();
    let baseline = apply(&before, table, rect);
    let failed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::IsolateTableRect { table, rect })
        .with_step(TransactionStep::RemoveNode {
            node: before.root(),
        })
        .apply_with_changes(&before);
    assert_eq!(failed.unwrap_err(), Error::InvalidTransaction);
    let repeated = apply(&before, table, rect);
    assert_eq!(repeated.document().store(), baseline.document().store());
    assert_eq!(
        repeated.document().next_node_id(),
        baseline.document().next_node_id()
    );
}
