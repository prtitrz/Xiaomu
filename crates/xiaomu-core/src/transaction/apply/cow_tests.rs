use super::*;
use crate::document::{AttrValue, NodeStoreBuilder};
use std::collections::BTreeMap;

#[test]
fn multiple_transaction_steps_and_table_batches_reuse_one_private_map() {
    let mut builder = NodeStoreBuilder::new();
    let p = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([p]),
        )
        .unwrap();
    let next_node_id = builder.peek_next_id().raw();
    let original = builder.finish();
    let mut context = ApplyContext {
        root,
        store: original.clone(),
        next_node_id,
    };
    context
        .apply_set_node_attrs(
            p,
            &NodeAttrs::new(BTreeMap::from([("value".into(), AttrValue::Integer(0))])).unwrap(),
        )
        .unwrap();
    let separated = context.store.map_storage_id();
    assert_ne!(separated, original.map_storage_id());
    for value in 1..30 {
        context
            .apply_set_node_attrs(
                p,
                &NodeAttrs::new(BTreeMap::from([(
                    "value".into(),
                    AttrValue::Integer(value),
                )]))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(context.store.map_storage_id(), separated);
    }
    let (maps, _) = context.apply_insert_table(root, 1, 2, 2).unwrap();
    let StepMap::NodeInserted {
        inserted: table, ..
    } = maps[0]
    else {
        panic!("table map")
    };
    assert_eq!(context.store.map_storage_id(), separated);
    let (maps, _) = context
        .apply_merge_table_cells(table, crate::document::TableRect::new(0, 0, 2, 2).unwrap())
        .unwrap();
    assert!(!maps.is_empty());
    assert_eq!(context.store.map_storage_id(), separated);
    context.apply_remove_node(table).unwrap();
    assert_eq!(context.store.map_storage_id(), separated);
    assert!(original.get(p).unwrap().attrs().is_empty());
    assert_eq!(original.len(), 2);
}

#[test]
fn exhausted_allocation_fails_before_touching_shared_store() {
    let mut builder = NodeStoreBuilder::new();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let original = builder.finish();
    let mut context = ApplyContext {
        root,
        store: original.clone(),
        next_node_id: u64::MAX,
    };
    assert_eq!(
        context
            .allocate_node(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline()
            )
            .unwrap_err(),
        Error::NodeIdExhausted
    );
    assert_eq!(context.store.map_storage_id(), original.map_storage_id());
    assert_eq!(context.store, original);
    assert_eq!(context.next_node_id, u64::MAX);
}
