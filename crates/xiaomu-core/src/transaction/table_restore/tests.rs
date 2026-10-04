use super::*;
use crate::document::{NodeAttrs, NodeContent, NodeStoreBuilder};

fn fixture() -> (NodeStore, NodeId, NodeId, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let p = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let q = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let a = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([p]),
        )
        .unwrap();
    let b = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([q]),
        )
        .unwrap();
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([a, b]),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    (builder.finish(), table, row, a, b, p)
}

#[test]
fn overlay_parent_capture_uses_replacement_references_and_keeps_original_store_untouched() {
    let (store, table, row, a, b, p) = fixture();
    let before = store.map_storage_id();
    let old_row = store.get(row).unwrap();
    let old_b = store.get(b).unwrap();
    let mut blocks = old_b.content().as_children().unwrap().to_vec();
    blocks.push(p);
    let expected = vec![
        old_row.clone(),
        store.get(a).unwrap().clone(),
        old_b.clone(),
        store.get(p).unwrap().clone(),
    ];
    let replacement = vec![
        old_row.with_content(NodeContent::children([b])).unwrap(),
        old_b.with_content(NodeContent::children(blocks)).unwrap(),
        store.get(p).unwrap().clone(),
    ];
    let overlay = expected_parents_after_exchange(&store, table, &expected, &replacement).unwrap();
    assert_eq!(overlay.get(&p), Some(&b));
    assert_eq!(overlay.get(&b), Some(&row));
    assert_eq!(overlay.get(&row), Some(&table));
    let old = expected_parents(&store, table, &[store.get(p).unwrap().clone()]).unwrap();
    assert_eq!(old.get(&p), Some(&a));
    assert_eq!(store.map_storage_id(), before);
    let mut after = store.clone();
    after.exchange_mut(&expected, &replacement).unwrap();
    assert_eq!(
        expected_parents(&after, table, &replacement).unwrap(),
        overlay
    );
    assert_eq!(store.map_storage_id(), before);
    assert!(store.contains(a));
}

#[test]
fn overlay_rejects_removed_references_before_any_exchange() {
    let (store, table, row, a, _, _) = fixture();
    let before = store.map_storage_id();
    let expected = vec![store.get(a).unwrap().clone()];
    let replacement = vec![store.get(row).unwrap().clone()];
    assert_eq!(
        expected_parents_after_exchange(&store, table, &expected, &replacement).unwrap_err(),
        Error::UnknownNode
    );
    assert_eq!(store.map_storage_id(), before);
    assert!(store.contains(a));
}
