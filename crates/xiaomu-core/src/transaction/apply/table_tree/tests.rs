use super::*;
use crate::document::{AttrValue, NodeAttrs, NodeStoreBuilder, XiaomuDocument};
use crate::text::TextOffset;
use crate::transaction::{Transaction, TransactionOrigin};
use std::collections::BTreeMap;
use std::sync::Arc;

fn document(columns: i64, nested: bool) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let p = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let mut blocks = vec![p];
    if nested {
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
                NodeAttrs::empty(),
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
        blocks.push(
            builder
                .insert(
                    NodeKind::Table,
                    NodeAttrs::empty(),
                    NodeContent::children([row]),
                )
                .unwrap(),
        );
    }
    let cell = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::new(BTreeMap::from([(
                "colspan".into(),
                AttrValue::Integer(columns),
            )]))
            .unwrap(),
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
    (XiaomuDocument::new(root, builder.finish()).unwrap(), table)
}

fn context(document: &XiaomuDocument) -> ApplyContext {
    ApplyContext {
        root: document.root(),
        store: document.store().clone(),
        next_node_id: document.next_node_id(),
    }
}

#[test]
fn materialization_checks_entire_fresh_identity_range_before_mutation() {
    let (document, table) = document(2, true);
    let template = TableTreeTemplate::capture(&document, table).unwrap();
    let mut context = context(&document);
    context.next_node_id = u64::MAX - 2;
    let before = context.store.clone();
    assert_eq!(
        context
            .apply_insert_table_tree(context.root, 1, &template)
            .unwrap_err(),
        Error::NodeIdExhausted
    );
    assert_eq!(context.store, before);
    assert_eq!(context.next_node_id, u64::MAX - 2);
}

#[test]
fn materialization_accounts_for_template_nested_and_existing_target_grids() {
    let (source, table) = document(500_000, true);
    let template = TableTreeTemplate::capture(&source, table).unwrap();
    let (target, _) = document(500_000, false);
    let mut context = context(&target);
    let before = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context
            .apply_insert_table_tree(context.root, 1, &template)
            .unwrap_err(),
        Error::TableResourceLimit
    );
    assert_eq!(context.store, before);
    assert_eq!(context.next_node_id, ceiling);
}

#[test]
fn missing_or_escaped_local_references_do_not_reach_destination_ids() {
    for inline in [false, true] {
        let (document, table) = document(2, false);
        let mut tree = TableTreeTemplate::capture(&document, table).unwrap();
        let data = Arc::get_mut(&mut tree.data).unwrap();
        if inline {
            let node = data
                .nodes
                .iter_mut()
                .find(|node| matches!(node.content, TemplateContent::Inline { .. }))
                .unwrap();
            let TemplateContent::Inline { atoms, .. } = &mut node.content else {
                unreachable!()
            };
            atoms.push((usize::MAX, TextOffset::ZERO));
        } else {
            data.nodes[0].content = TemplateContent::Children(vec![usize::MAX]);
        }
        let mut context = context(&document);
        let before = context.store.clone();
        let ceiling = context.next_node_id;
        assert_eq!(
            context
                .apply_insert_table_tree(context.root, 1, &tree)
                .unwrap_err(),
            Error::InvalidTransaction
        );
        assert_eq!(context.store, before);
        assert_eq!(context.next_node_id, ceiling);
    }
}

#[test]
fn duplicate_inline_references_fail_before_store_commit() {
    let (document, table) = document(2, false);
    let mut tree = TableTreeTemplate::capture(&document, table).unwrap();
    let data = Arc::get_mut(&mut tree.data).unwrap();
    let node = data
        .nodes
        .iter_mut()
        .find(|node| matches!(node.content, TemplateContent::Inline { .. }))
        .unwrap();
    let TemplateContent::Inline { atoms, .. } = &mut node.content else {
        unreachable!()
    };
    atoms.extend([(0, TextOffset::ZERO), (0, TextOffset::ZERO)]);
    let mut context = context(&document);
    let before = context.store.clone();
    let ceiling = context.next_node_id;
    assert_eq!(
        context
            .apply_insert_table_tree(context.root, 1, &tree)
            .unwrap_err(),
        Error::DuplicateInlineAtomReference
    );
    assert_eq!(context.store, before);
    assert_eq!(context.next_node_id, ceiling);
}

#[test]
fn full_validation_rejects_wrong_kind_local_atom_target_without_publishing_state() {
    let (document, table) = document(2, false);
    let mut tree = TableTreeTemplate::capture(&document, table).unwrap();
    let data = Arc::get_mut(&mut tree.data).unwrap();
    let node = data
        .nodes
        .iter_mut()
        .find(|node| matches!(node.content, TemplateContent::Inline { .. }))
        .unwrap();
    let TemplateContent::Inline { atoms, .. } = &mut node.content else {
        unreachable!()
    };
    atoms.push((0, TextOffset::ZERO));
    let before = document.store().clone();
    assert_eq!(
        Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertTableTree {
                parent: document.root(),
                index: 1,
                tree
            })
            .apply_with_changes(&document)
            .unwrap_err(),
        Error::InvalidInlineAtomReference
    );
    assert_eq!(document.store(), &before);
}
