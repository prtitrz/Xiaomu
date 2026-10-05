//! Exact allocator-capacity query, including the post-Undo high-water mark.
use super::*;
use crate::document::{NodeAttrs, NodeContent, NodeStoreBuilder};
use crate::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn document() -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
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
            NodeContent::children([paragraph]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

#[test]
fn allocation_query_matches_checked_add_at_the_exact_boundary_without_mutation() {
    let base = document();
    for next in [base.next_node_id, u64::MAX - 3, u64::MAX - 1, u64::MAX] {
        let mut document = base.clone();
        document.next_node_id = next;
        let before = document.clone();
        for count in [0usize, 1, 2, 3, 4, usize::MAX] {
            let expected = u64::try_from(count)
                .ok()
                .and_then(|count| next.checked_add(count))
                .is_some();
            assert_eq!(document.can_allocate_node_ids(count), expected);
            assert_eq!(document.clone().can_allocate_node_ids(count), expected);
        }
        assert_eq!(document.next_node_id, before.next_node_id);
        assert_eq!(document.revision(), before.revision());
        assert_eq!(document.root(), before.root());
        assert_eq!(document.store(), before.store());
        for node in before.store().iter() {
            assert!(document.store.shares_node_payload(&before.store, node.id()));
        }
    }
}

#[test]
fn undo_restores_ids_but_does_not_restore_allocation_capacity() {
    let mut before = document();
    before.next_node_id = u64::MAX - 1;
    assert!(before.can_allocate_node_ids(1));
    assert!(!before.can_allocate_node_ids(2));
    let edit =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::InsertNode {
            parent: before.root(),
            index: 0,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        });
    let applied = edit.apply_with_changes(&before).unwrap();
    assert!(applied.document().can_allocate_node_ids(0));
    assert!(!applied.document().can_allocate_node_ids(1));
    let undone = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undone.document().store(), before.store());
    assert!(!undone.document().can_allocate_node_ids(1));
    let redone = undone.inverse().apply(undone.document()).unwrap();
    assert_eq!(redone.store(), applied.document().store());
    assert!(!redone.can_allocate_node_ids(1));
    assert!(before.can_allocate_node_ids(1));
    assert!(edit.apply(applied.document()).is_err());
    assert_eq!(applied.document().next_node_id, u64::MAX);
}
