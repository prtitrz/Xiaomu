use super::*;
use crate::document::NodeStoreBuilder;

fn context(columns: &[i64]) -> (ApplyContext, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let mut tables = Vec::new();
    for columns in columns {
        let p = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let cell = builder
            .insert(
                NodeKind::TableCell,
                NodeAttrs::new(BTreeMap::from([(
                    "colspan".into(),
                    AttrValue::Integer(*columns),
                )]))
                .unwrap(),
                NodeContent::children([p]),
            )
            .unwrap();
        let row = builder
            .insert(
                NodeKind::TableRow,
                NodeAttrs::empty(),
                NodeContent::children([cell]),
            )
            .unwrap();
        tables.push(
            builder
                .insert(
                    NodeKind::Table,
                    NodeAttrs::empty(),
                    NodeContent::children([row]),
                )
                .unwrap(),
        );
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(tables.clone()),
        )
        .unwrap();
    let next_node_id = builder.peek_next_id().raw();
    (
        ApplyContext {
            root,
            store: builder.finish(),
            next_node_id,
        },
        tables,
    )
}

#[test]
fn logical_insertions_check_all_required_ids_before_mutating() {
    for row in [false, true] {
        let (mut context, tables) = context(&[1]);
        context.next_node_id = u64::MAX - 1;
        let store = context.store.clone();
        let result = if row {
            context.apply_insert_table_row_logical(tables[0], 1, &[NodeKind::TableCell])
        } else {
            context.apply_insert_table_column_logical(tables[0], 1, &[NodeKind::TableCell])
        };
        assert_eq!(result.unwrap_err(), Error::NodeIdExhausted);
        assert_eq!(context.store, store);
        assert_eq!(context.next_node_id, u64::MAX - 1);
    }
}

#[test]
fn span_only_column_growth_still_checks_other_tables_aggregate_grid_budget() {
    let (mut context, tables) = context(&[600_000, 400_000]);
    let store = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context
            .apply_insert_table_column_logical(tables[0], 1, &[NodeKind::TableCell])
            .unwrap_err(),
        Error::TableResourceLimit
    );
    assert_eq!(context.store, store);
    assert_eq!(context.next_node_id, ceiling);
}

#[test]
fn all_covered_column_insertion_allocates_no_nodes_or_id() {
    let (mut context, tables) = context(&[2]);
    context.next_node_id = u64::MAX;
    let before = context.store.len();
    context
        .apply_insert_table_column_logical(tables[0], 1, &[NodeKind::TableCell])
        .unwrap();
    assert_eq!(context.next_node_id, u64::MAX);
    assert_eq!(context.store.len(), before);
}
