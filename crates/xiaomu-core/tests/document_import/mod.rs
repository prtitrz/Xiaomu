//! Exact fresh-ID document copying, mapping, lineage and history contracts.

mod support;

use std::collections::{BTreeMap, BTreeSet};

use xiaomu_core::Error;
use xiaomu_core::document::{
    AttrValue, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition, StepMap};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap, NodeSelection};
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_core::transaction::{
    AppliedTransaction, DocumentTemplate, Transaction, TransactionOrigin, TransactionStep,
};

use support::{attrs, fixture, paragraph};

fn replace(target: &XiaomuDocument, source: &XiaomuDocument) -> AppliedTransaction {
    Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::ReplaceDocument {
            template: DocumentTemplate::capture(source).unwrap(),
        })
        .apply_with_changes(target)
        .unwrap()
}

fn compare_copy(
    source: &XiaomuDocument,
    old: NodeId,
    after: &XiaomuDocument,
    new: NodeId,
    pairs: &mut BTreeMap<NodeId, NodeId>,
) {
    assert!(pairs.insert(old, new).is_none());
    let a = source.node(old).unwrap();
    let b = after.node(new).unwrap();
    assert_eq!(a.kind(), b.kind());
    assert_eq!(a.attrs(), b.attrs());
    match (a.content(), b.content()) {
        (NodeContent::Children(a), NodeContent::Children(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                compare_copy(source, *a, after, *b, pairs);
            }
        }
        (NodeContent::Inline(a), NodeContent::Inline(b)) => {
            assert_eq!(a.runs(), b.runs());
            assert_eq!(a.atoms().len(), b.atoms().len());
            for (a, b) in a.atoms().iter().zip(b.atoms()) {
                assert_eq!(a.text_offset(), b.text_offset());
                compare_copy(source, a.atom(), after, b.atom(), pairs);
            }
        }
        (NodeContent::InlineAtom(a), NodeContent::InlineAtom(b)) => assert_eq!(a, b),
        (NodeContent::Atomic, NodeContent::Atomic) => {}
        other => panic!("shape changed: {other:?}"),
    }
}

#[test]
fn every_kind_raw_attrs_marks_atoms_and_nested_spans_copy_with_fresh_receiver_ids() {
    let (source, source_table, _) = fixture();
    // A distinct smaller lineage deliberately has colliding raw source IDs.
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "receiver");
    let root = builder
        .insert(
            NodeKind::Document,
            attrs(&[("old", AttrValue::Bool(true))]),
            NodeContent::children([p]),
        )
        .unwrap();
    let next = builder.peek_next_id();
    let target = XiaomuDocument::new(root, builder.finish()).unwrap();
    let template = DocumentTemplate::capture(&source).unwrap();
    assert_eq!(template.node_count() + 1, source.node_count());
    assert!(template.payload_bytes() > 0);
    assert_eq!(template, template.clone());
    let applied = replace(&target, &source);
    assert_eq!(applied.document().root(), root);
    assert_eq!(
        applied.document().revision().as_u64(),
        target.revision().as_u64() + 1
    );
    let mut pairs = BTreeMap::new();
    compare_copy(&source, source.root(), applied.document(), root, &mut pairs);
    assert_eq!(pairs.len(), source.node_count());
    assert!(pairs.values().copied().collect::<BTreeSet<_>>().len() == pairs.len());
    assert!(
        pairs
            .values()
            .filter(|id| **id != root)
            .all(|id| target.node(*id).is_none())
    );
    let new_children = applied
        .document()
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(new_children[0], next);
    let table = pairs[&source_table];
    let grid = applied.document().table_grid(table).unwrap();
    assert_eq!(
        (grid.rows(), grid.columns(), grid.origins().len()),
        (2, 2, 1)
    );
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undo.document().store(), target.store());
    let redo = undo.inverse().apply_with_changes(undo.document()).unwrap();
    assert_eq!(redo.document().store(), applied.document().store());
}

#[test]
fn all_old_endpoints_and_atoms_are_deleted_and_root_gaps_follow_real_child_edits() {
    let (target, _, outside) = fixture();
    let source = empty();
    let applied = replace(&target, &source);
    assert!(!applied.changes().steps().is_empty());
    for node in target
        .store()
        .iter()
        .filter(|node| node.id() != target.root())
    {
        assert_eq!(
            applied
                .changes()
                .map_node_selection(NodeSelection::new(node.id())),
            MappedPosition::Deleted
        );
    }
    let point = InlinePoint::new(outside, TextOffset::ZERO, 0, CursorAffinity::Before);
    assert_eq!(
        applied.changes().map_inline_point(point, MapBias::End),
        MappedPosition::Deleted
    );
    let old_children = target
        .node(target.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(applied.changes().steps().len(), old_children.len());
    for index in 0..=old_children.len() {
        for bias in [MapBias::Start, MapBias::End] {
            assert_eq!(
                applied
                    .changes()
                    .map_node_gap(NodeGap::new(target.root(), index), bias),
                MappedPosition::Mapped(NodeGap::new(target.root(), 0))
            );
        }
    }
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undo.document().store(), target.store());
    assert_eq!(
        undo.changes()
            .map_node_gap(NodeGap::new(target.root(), 0), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(target.root(), old_children.len()))
    );
    assert!(
        undo.changes()
            .steps()
            .iter()
            .all(|step| matches!(step, StepMap::NodeInserted { .. }))
    );
}

#[test]
fn earlier_edit_undo_redo_rebuilds_equal_store_without_invalidating_document_redo() {
    let (before, _, text) = fixture();
    let range = TextRange::new(TextOffset::ZERO, TextOffset::ZERO).unwrap();
    let edit = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceText {
            node: text,
            range,
            replacement: "earlier ".into(),
        })
        .apply_with_changes(&before)
        .unwrap();
    let imported = replace(edit.document(), &empty());
    let undo_import = imported
        .inverse()
        .apply_with_changes(imported.document())
        .unwrap();
    assert_eq!(undo_import.document().store(), edit.document().store());
    let undo_edit = edit
        .inverse()
        .apply_with_changes(undo_import.document())
        .unwrap();
    assert_eq!(undo_edit.document().store(), before.store());
    let redo_edit = undo_edit
        .inverse()
        .apply_with_changes(undo_edit.document())
        .unwrap();
    assert_eq!(redo_edit.document().store(), edit.document().store());
    let redo_import = undo_import
        .inverse()
        .apply_with_changes(redo_edit.document())
        .unwrap();
    assert_eq!(redo_import.document().store(), imported.document().store());
}

#[test]
fn restore_rejects_foreign_identical_store_and_stale_current_payload() {
    let (before, _, _) = fixture();
    let imported = replace(&before, &before);
    let foreign = XiaomuDocument::new(
        imported.document().root(),
        imported.document().store().clone(),
    )
    .unwrap();
    assert_eq!(foreign.store(), imported.document().store());
    assert_eq!(
        imported.inverse().apply(&foreign).unwrap_err(),
        Error::InvalidTransaction
    );
    let stale = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: before.root(),
            attrs: NodeAttrs::empty(),
        })
        .apply(imported.document())
        .unwrap();
    assert_eq!(
        imported.inverse().apply(&stale).unwrap_err(),
        Error::InvalidTransaction
    );
    assert_eq!(
        imported
            .inverse()
            .apply(&imported.document().clone())
            .unwrap()
            .store(),
        before.store()
    );
}

#[test]
fn empty_root_attrs_copy_exactly_and_repeated_imports_never_reuse_retired_ids() {
    let (before, _, _) = fixture();
    let first = replace(&before, &before);
    let first_ids: BTreeSet<_> = first
        .document()
        .store()
        .iter()
        .filter(|n| n.id() != before.root())
        .map(|n| n.id())
        .collect();
    let undo = first
        .inverse()
        .apply_with_changes(first.document())
        .unwrap();
    let second = replace(undo.document(), &before);
    assert!(
        second
            .document()
            .store()
            .iter()
            .filter(|n| n.id() != before.root())
            .all(|n| !first_ids.contains(&n.id()))
    );
    let cleared = replace(second.document(), &empty());
    assert!(
        cleared
            .document()
            .node(before.root())
            .unwrap()
            .attrs()
            .is_empty()
    );
    assert_eq!(cleared.document().node_count(), 1);
    assert_eq!(
        cleared.inverse().apply(cleared.document()).unwrap().store(),
        second.document().store()
    );
}

#[test]
fn late_step_failure_leaves_input_and_allocation_unchanged() {
    let (before, _, _) = fixture();
    let template = DocumentTemplate::capture(&before).unwrap();
    let failed = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::ReplaceDocument { template })
        .with_step(TransactionStep::RemoveNode {
            node: before.root(),
        })
        .apply(&before);
    assert_eq!(failed.unwrap_err(), Error::InvalidTransaction);
    assert_eq!(
        replace(&before, &before).document().store(),
        replace(&before, &before).document().store()
    );
}

fn empty() -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

#[test]
fn source_or_retained_old_tree_depth_is_bounded_before_any_change() {
    let mut builder = NodeStoreBuilder::new();
    let mut block = paragraph(&mut builder, "deep");
    for _ in 0..128 {
        block = builder
            .insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([block]),
            )
            .unwrap();
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap();
    let deep = XiaomuDocument::new(root, builder.finish()).unwrap();
    assert_eq!(
        DocumentTemplate::capture(&deep).unwrap_err(),
        Error::SnapshotResourceLimit
    );
    let template = DocumentTemplate::capture(&empty()).unwrap();
    let before = deep.store().clone();
    assert_eq!(
        Transaction::new(TransactionOrigin::System)
            .with_step(TransactionStep::ReplaceDocument { template })
            .apply(&deep)
            .unwrap_err(),
        Error::SnapshotResourceLimit
    );
    assert_eq!(deep.store(), &before);
}
