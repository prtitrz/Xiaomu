use super::*;
use crate::document::{AttrValue, NodeAttrs, NodeContent, NodeStoreBuilder, XiaomuDocument};
use crate::transaction::{Transaction, TransactionOrigin};
use std::collections::BTreeMap;
use std::sync::Arc;

fn document(with_paragraph: bool) -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let children = if with_paragraph {
        vec![
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::empty_inline(),
                )
                .unwrap(),
        ]
    } else {
        vec![]
    };
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

fn context(doc: &XiaomuDocument, next_node_id: u64) -> ApplyContext {
    ApplyContext {
        root: doc.root(),
        store: doc.store().clone(),
        next_node_id,
    }
}

fn with_ceiling(doc: &XiaomuDocument, next: u64) -> XiaomuDocument {
    XiaomuDocument::from_applied_parts(
        doc.version(),
        doc.revision(),
        doc.root(),
        doc.store().clone(),
        next,
        doc.lineage().clone(),
    )
    .unwrap()
}

fn transaction(template: DocumentTemplate) -> Transaction {
    Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::ReplaceDocument { template })
}

#[test]
fn allocation_exhaustion_is_checked_before_cow_and_zero_new_ids_still_fit() {
    let before = document(true);
    let mut context = context(&before, u64::MAX);
    let storage = context.store.map_storage_id();
    assert_eq!(
        context
            .apply_replace_document(
                &DocumentTemplate::capture(&before).unwrap(),
                before.lineage(),
                &mut SnapshotBudget::default()
            )
            .unwrap_err(),
        Error::NodeIdExhausted
    );
    assert_eq!(context.next_node_id, u64::MAX);
    assert_eq!(context.store, *before.store());
    assert_eq!(context.store.map_storage_id(), storage);
    context
        .apply_replace_document(
            &DocumentTemplate::capture(&document(false)).unwrap(),
            before.lineage(),
            &mut SnapshotBudget::default(),
        )
        .unwrap();
    assert_eq!(context.next_node_id, u64::MAX);
    assert_eq!(context.store.len(), 1);
}

#[test]
fn high_water_is_exact_and_unchanged_across_repeated_undo_redo() {
    let before = with_ceiling(&document(true), u64::MAX - 1);
    let imported = transaction(DocumentTemplate::capture(&document(true)).unwrap())
        .apply_with_changes(&before)
        .unwrap();
    assert_eq!(imported.document().next_node_id(), u64::MAX);
    let mut current = imported.document().clone();
    let mut next = imported.inverse().clone();
    for index in 0..8 {
        let applied = next.apply_with_changes(&current).unwrap();
        assert_eq!(applied.document().next_node_id(), u64::MAX);
        assert_eq!(
            applied.document().store(),
            if index % 2 == 0 {
                before.store()
            } else {
                imported.document().store()
            }
        );
        next = applied.inverse().clone();
        current = applied.into_document();
    }
    let empty = transaction(DocumentTemplate::capture(&document(false)).unwrap())
        .apply_with_changes(&current)
        .unwrap();
    assert_eq!(empty.document().next_node_id(), u64::MAX);
    assert_eq!(
        empty.inverse().apply(empty.document()).unwrap().store(),
        current.store()
    );
}

#[test]
fn malformed_private_template_root_or_reference_is_rejected_without_mutation() {
    let before = document(true);
    for root_kind in [false, true] {
        let mut template = DocumentTemplate::capture(&before).unwrap();
        let data = Arc::get_mut(&mut template.data).unwrap();
        if root_kind {
            data.nodes[0].kind = NodeKind::Paragraph;
        } else {
            data.nodes[0].content = TemplateContent::Children(vec![usize::MAX]);
        }
        let mut context = context(&before, before.next_node_id());
        let storage = context.store.map_storage_id();
        assert_eq!(
            context
                .apply_replace_document(&template, before.lineage(), &mut SnapshotBudget::default())
                .unwrap_err(),
            if root_kind {
                Error::InvalidRootNode
            } else {
                Error::InvalidTransaction
            }
        );
        assert_eq!(context.store, *before.store());
        assert_eq!(context.store.map_storage_id(), storage);
        assert_eq!(context.next_node_id, before.next_node_id());
    }
}

#[test]
fn retained_inverse_attrs_are_budgeted_before_destination_materialization() {
    let before = document(true);
    let mut value = AttrValue::Null;
    for _ in 0..64 {
        value = AttrValue::List(vec![value]);
    }
    let before = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: before.root(),
            attrs: NodeAttrs::new(BTreeMap::from([("deep".into(), value)])).unwrap(),
        })
        .apply(&before)
        .unwrap();
    let mut context = context(&before, before.next_node_id());
    let storage = context.store.map_storage_id();
    let template = DocumentTemplate::capture(&document(true)).unwrap();
    assert_eq!(
        context
            .apply_replace_document(&template, before.lineage(), &mut SnapshotBudget::default())
            .unwrap_err(),
        Error::SnapshotResourceLimit
    );
    assert_eq!(context.store.map_storage_id(), storage);
    assert_eq!(context.store, *before.store());
    assert_eq!(context.next_node_id, before.next_node_id());
}

#[test]
fn restore_root_lineage_payload_and_allocator_guards_are_independent() {
    let before = document(true);
    let applied = transaction(DocumentTemplate::capture(&before).unwrap())
        .apply_with_changes(&before)
        .unwrap();
    let TransactionStep::RestoreDocument { restore } = &applied.inverse().steps()[0] else {
        panic!("restore")
    };
    for choice in 0..4 {
        let mut context = context(applied.document(), applied.document().next_node_id());
        let foreign = document(true);
        let lineage = if choice == 0 {
            foreign.lineage()
        } else {
            before.lineage()
        };
        if choice == 1 {
            context.root = before
                .store()
                .iter()
                .find(|node| node.id() != before.root())
                .unwrap()
                .id();
        }
        if choice == 2 {
            context.next_node_id -= 1;
        }
        if choice == 3 {
            context.store = before.store().clone();
        }
        let old = context.store.clone();
        let ceiling = context.next_node_id;
        assert_eq!(
            context
                .apply_restore_document(restore, lineage, &mut SnapshotBudget::default())
                .unwrap_err(),
            Error::InvalidTransaction
        );
        assert_eq!(context.store, old);
        assert_eq!(context.next_node_id, ceiling);
    }
}

#[test]
fn transaction_wide_snapshot_step_limit_covers_replace_and_restore() {
    let before = document(false);
    let replace = TransactionStep::ReplaceDocument {
        template: DocumentTemplate::capture(&before).unwrap(),
    };
    let applied = transaction(DocumentTemplate::capture(&before).unwrap())
        .apply_with_changes(&before)
        .unwrap();
    let restore = applied.inverse().steps()[0].clone();
    for (snapshot, step) in [(&before, replace), (applied.document(), restore)] {
        let mut bounded = Transaction::new(TransactionOrigin::System);
        for _ in 0..64 {
            bounded.push_step(step.clone());
        }
        let accepted = bounded.apply_with_changes(snapshot).unwrap();
        assert_eq!(accepted.document().store(), snapshot.store());
        assert_eq!(accepted.inverse().steps().len(), 64);
        accepted.inverse().apply(accepted.document()).unwrap();
        bounded.push_step(step);
        assert_eq!(
            bounded.apply(snapshot).unwrap_err(),
            Error::SnapshotResourceLimit
        );
        assert_eq!(snapshot.store(), before.store());
    }
}

#[test]
fn multiple_individually_admitted_snapshots_share_one_owned_payload_budget() {
    let before = document(false);
    let before = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::SetNodeAttrs {
            node: before.root(),
            attrs: NodeAttrs::new(BTreeMap::from([(
                "many".into(),
                AttrValue::List(vec![AttrValue::Null; 10_000]),
            )]))
            .unwrap(),
        })
        .apply(&before)
        .unwrap();
    let template = DocumentTemplate::capture(&before).unwrap();
    let single = transaction(template.clone())
        .apply_with_changes(&before)
        .unwrap();
    for (snapshot, step) in [
        (&before, TransactionStep::ReplaceDocument { template }),
        (single.document(), single.inverse().steps()[0].clone()),
    ] {
        let mut many = Transaction::new(TransactionOrigin::System);
        for _ in 0..60 {
            many.push_step(step.clone());
        }
        assert_eq!(
            many.apply(snapshot).unwrap_err(),
            Error::SnapshotResourceLimit
        );
        assert_eq!(snapshot.store(), before.store());
        // The same bounded step still succeeds when it is the only operation.
        Transaction::new(TransactionOrigin::System)
            .with_step(step)
            .apply(snapshot)
            .unwrap();
    }
}
