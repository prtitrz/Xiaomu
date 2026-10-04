use super::*;

fn fixture() -> (NodeStore, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let second = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    (builder.finish(), first, second)
}

#[test]
fn working_map_separates_once_and_keeps_unchanged_payloads_shared() {
    let (original, first, second) = fixture();
    let mut working = original.clone();
    assert_eq!(working.map_storage_id(), original.map_storage_id());
    working
        .replace_node_mut(original.get(first).unwrap().clone())
        .unwrap();
    let separated = working.map_storage_id();
    assert_ne!(separated, original.map_storage_id());
    assert!(working.shares_node_payload(&original, second));
    for _ in 0..20 {
        working
            .replace_node_mut(original.get(first).unwrap().clone())
            .unwrap();
        assert_eq!(working.map_storage_id(), separated);
    }
    let inserted = Node::new(
        NodeId::from_allocated(10),
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        NodeContent::empty_inline(),
    )
    .unwrap();
    working.insert_node_mut(inserted.clone()).unwrap();
    assert_eq!(working.map_storage_id(), separated);
    working.remove_nodes_mut(&BTreeSet::from([inserted.id()]));
    assert_eq!(working.map_storage_id(), separated);
    assert_eq!(working, original);
    assert!(!original.contains(inserted.id()));
}

#[test]
fn failed_preconditions_do_not_separate_a_shared_map() {
    let (original, first, _) = fixture();
    let mut working = original.clone();
    let missing = Node::new(
        NodeId::from_allocated(10),
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        NodeContent::empty_inline(),
    )
    .unwrap();
    assert_eq!(
        working.replace_node_mut(missing.clone()),
        Err(Error::UnknownNode)
    );
    assert_eq!(
        working.insert_node_mut(original.get(first).unwrap().clone()),
        Err(Error::DuplicateChildReference)
    );
    assert_eq!(
        working.exchange_mut(std::slice::from_ref(&missing), &[]),
        Err(Error::InvalidTransaction)
    );
    assert_eq!(
        working.exchange_mut(&[], &[original.get(first).unwrap().clone()]),
        Err(Error::InvalidTransaction)
    );
    assert_eq!(
        working.exchange_mut(&[], &[missing.clone(), missing.clone()]),
        Err(Error::InvalidTransaction)
    );
    working.remove_nodes_mut(&BTreeSet::from([missing.id()]));
    assert_eq!(working.map_storage_id(), original.map_storage_id());
    assert_eq!(working, original);
}

#[test]
fn exchange_after_an_ordinary_write_reuses_the_working_map_and_protects_snapshots() {
    let (original, first, second) = fixture();
    let mut working = original.clone();
    working
        .replace_node_mut(original.get(first).unwrap().clone())
        .unwrap();
    let storage = working.map_storage_id();
    let fresh = Node::new(
        NodeId::from_allocated(10),
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        NodeContent::empty_inline(),
    )
    .unwrap();
    working
        .exchange_mut(
            &[original.get(first).unwrap().clone()],
            std::slice::from_ref(&fresh),
        )
        .unwrap();
    assert_eq!(working.map_storage_id(), storage);
    assert!(original.contains(first));
    assert!(!original.contains(fresh.id()));
    assert!(working.shares_node_payload(&original, second));
    let published = working.clone();
    working.remove_nodes_mut(&BTreeSet::from([second]));
    assert_ne!(working.map_storage_id(), published.map_storage_id());
    assert!(published.contains(second));
}
