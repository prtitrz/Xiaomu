use super::*;
use crate::document::NodeStoreBuilder;

fn spanning_context(columns: i64) -> (ApplyContext, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
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
                AttrValue::Integer(columns),
            )]))
            .unwrap(),
            NodeContent::children([paragraph]),
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
    let next_node_id = builder.peek_next_id().raw();
    (
        ApplyContext {
            root,
            store: builder.finish(),
            next_node_id,
        },
        table,
        cell,
    )
}

#[test]
fn split_checks_complete_identity_range_before_any_mutation() {
    let (mut context, table, cell) = spanning_context(2);
    context.next_node_id = u64::MAX - 1;
    let store = context.store.clone();
    assert_eq!(
        context.apply_split_table_cell(table, cell).unwrap_err(),
        Error::NodeIdExhausted
    );
    assert_eq!(context.store, store);
    assert_eq!(context.next_node_id, u64::MAX - 1);
}

#[test]
fn split_checks_physical_cell_budget_before_any_mutation() {
    let (mut context, table, cell) = spanning_context(100_001);
    let store = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context.apply_split_table_cell(table, cell).unwrap_err(),
        Error::TableResourceLimit
    );
    assert_eq!(context.store, store);
    assert_eq!(context.next_node_id, ceiling);
}

#[test]
fn inverse_rejects_duplicate_payloads_instead_of_trusting_opaque_container() {
    let (mut context, table, cell) = spanning_context(2);
    let (_, inverse) = context.apply_split_table_cell(table, cell).unwrap();
    let TransactionStep::RestoreTableCells { mut restore } = inverse[0].clone() else {
        panic!("expected exact inverse")
    };
    restore.replacement.push(restore.replacement[0].clone());
    let store = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context.apply_restore_table_cells(&restore).unwrap_err(),
        Error::InvalidTransaction
    );
    assert_eq!(context.store, store);
    assert_eq!(context.next_node_id, ceiling);
}

#[test]
fn batch_edit_shares_every_unchanged_content_payload() {
    let (mut context, table, cell) = spanning_context(3);
    let paragraph = context.children(cell).unwrap()[0];
    let before = context.store.clone();
    context.apply_split_table_cell(table, cell).unwrap();
    assert!(before.shares_node_payload(&context.store, paragraph));
    assert!(before.shares_node_payload(&context.store, table));
    assert!(before.shares_node_payload(&context.store, context.root));
}
