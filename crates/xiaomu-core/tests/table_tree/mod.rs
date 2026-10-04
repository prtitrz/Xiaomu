//! Whole-table template insertion and exact fresh-ID copy/undo contracts.

use std::collections::{BTreeMap, BTreeSet};

use xiaomu_core::Error;
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, LinkAttributes,
    LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    StringAttribute, TextRun, XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition, StepMap};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap, NodeSelection};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{
    AppliedTransaction, TableTreeTemplate, Transaction, TransactionOrigin, TransactionStep,
};

fn attrs(entries: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        entries
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone()))
            .collect(),
    )
    .unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn fixture() -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let link = Mark::Link(LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href(StringAttribute::Value("https://example.com/".into()))
            .with_title(StringAttribute::Null)
            .with_rel(StringAttribute::Value("nofollow custom".into())),
    ));
    let first_atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(
                InlineAtomContent::hard_break().with_marks(MarkSet::new([Mark::Bold]).unwrap()),
            ),
        )
        .unwrap();
    let second_atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
            attrs(&[("opaque", AttrValue::Null)]),
            NodeContent::InlineAtom(
                InlineAtomContent::new("@X")
                    .unwrap()
                    .with_marks(MarkSet::new([link.clone()]).unwrap()),
            ),
        )
        .unwrap();
    let text = InlineContent::new([
        TextRun::new("A🙂", MarkSet::new([Mark::Bold, link]).unwrap()).unwrap(),
        TextRun::new("中B", MarkSet::new([Mark::Italic]).unwrap()).unwrap(),
    ])
    .unwrap();
    let seam = text.offset_at(1).unwrap();
    let mixed = builder
        .insert(
            NodeKind::Paragraph,
            attrs(&[("alignment", AttrValue::Null)]),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    text.runs().iter().cloned(),
                    [
                        InlineAtomPlacement::new(first_atom, seam),
                        InlineAtomPlacement::new(second_atom, seam),
                    ],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let image = builder
        .insert(
            NodeKind::Image,
            attrs(&[
                ("asset", AttrValue::String("asset:private-image".into())),
                ("alt", AttrValue::String("picture".into())),
            ]),
            NodeContent::Atomic,
        )
        .unwrap();
    let nested_p = paragraph(&mut builder, "nested");
    let nested_cell = builder
        .insert(
            NodeKind::TableCell,
            attrs(&[("colwidth", AttrValue::Null)]),
            NodeContent::children([nested_p]),
        )
        .unwrap();
    let nested_row = builder
        .insert(
            NodeKind::TableRow,
            attrs(&[("nested-row", AttrValue::Bool(true))]),
            NodeContent::children([nested_cell]),
        )
        .unwrap();
    let nested = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([nested_row]),
        )
        .unwrap();
    let cell = builder
        .insert(
            NodeKind::TableHeader,
            attrs(&[
                ("rowspan", AttrValue::Integer(2)),
                ("colspan", AttrValue::Integer(2)),
                (
                    "colwidth",
                    AttrValue::List(vec![AttrValue::Integer(90), AttrValue::Integer(0)]),
                ),
                (
                    "opaque",
                    AttrValue::Object(BTreeMap::from([("k".into(), AttrValue::Null)])),
                ),
            ]),
            NodeContent::children([mixed, image, nested]),
        )
        .unwrap();
    let row0 = builder
        .insert(
            NodeKind::TableRow,
            attrs(&[("height", AttrValue::Integer(50))]),
            NodeContent::children([cell]),
        )
        .unwrap();
    let row1 = builder
        .insert(
            NodeKind::TableRow,
            attrs(&[("covered", AttrValue::Null)]),
            NodeContent::children([]),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            attrs(&[("layout", AttrValue::String("fixed".into()))]),
            NodeContent::children([row0, row1]),
        )
        .unwrap();
    let outside = paragraph(&mut builder, "outside");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table, outside]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        outside,
    )
}

fn insert(document: &XiaomuDocument, tree: TableTreeTemplate, index: usize) -> AppliedTransaction {
    Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableTree {
            parent: document.root(),
            index,
            tree,
        })
        .apply_with_changes(document)
        .unwrap()
}

fn inserted(applied: &AppliedTransaction) -> NodeId {
    let [StepMap::NodeInserted { inserted, .. }] = applied.changes().steps() else {
        panic!("one root insertion map")
    };
    *inserted
}

fn compare_copy(
    source: &XiaomuDocument,
    old: NodeId,
    after: &XiaomuDocument,
    new: NodeId,
    pairs: &mut BTreeMap<NodeId, NodeId>,
) {
    assert!(pairs.insert(old, new).is_none());
    assert!(
        source.node(new).is_none(),
        "every copied identity comes from the destination allocator"
    );
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

fn round_trip(before: &XiaomuDocument, applied: &AppliedTransaction) {
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undo.document().store(), before.store());
    let redo = undo.inverse().apply_with_changes(undo.document()).unwrap();
    assert_eq!(redo.document().store(), applied.document().store());
}

#[test]
fn template_copies_complete_spanning_rich_tree_with_fresh_ids_and_exact_inverse() {
    let (document, table, outside) = fixture();
    let template = TableTreeTemplate::capture(&document, table).unwrap();
    let copied_count = template.node_count();
    let applied = insert(&document, template, 1);
    let root = inserted(&applied);
    let mut pairs = BTreeMap::new();
    compare_copy(&document, table, applied.document(), root, &mut pairs);
    assert_eq!(pairs.len(), copied_count);
    assert_eq!(
        applied.document().store().len(),
        document.store().len() + copied_count
    );
    assert_eq!(
        applied.document().table_grid(root).unwrap().origins().len(),
        1
    );
    for (old, _) in pairs {
        assert_eq!(applied.document().node(old), document.node(old));
    }
    let point = InlinePoint::new(outside, TextOffset::ZERO, 0, CursorAffinity::After);
    assert_eq!(
        applied.changes().map_inline_point(point, MapBias::End),
        MappedPosition::Mapped(point)
    );
    assert_eq!(
        applied
            .changes()
            .map_node_gap(NodeGap::new(document.root(), 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(document.root(), 2))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_selection(NodeSelection::new(table)),
        MappedPosition::Mapped(NodeSelection::new(table))
    );
    round_trip(&document, &applied);
}

#[test]
fn repeated_use_of_one_template_allocates_disjoint_destination_subtrees() {
    let (document, table, _) = fixture();
    let template = TableTreeTemplate::capture(&document, table).unwrap();
    let first = insert(&document, template.clone(), 1);
    let second = insert(first.document(), template, 2);
    let mut pairs = BTreeMap::new();
    compare_copy(
        &document,
        table,
        first.document(),
        inserted(&first),
        &mut pairs,
    );
    let old: BTreeSet<_> = pairs.into_values().collect();
    let mut pairs = BTreeMap::new();
    compare_copy(
        first.document(),
        table,
        second.document(),
        inserted(&second),
        &mut pairs,
    );
    assert!(pairs.into_values().all(|id| !old.contains(&id)));
    round_trip(first.document(), &second);
}

#[test]
fn all_root_replacement_preserves_document_identity_and_undoes_old_tree_exactly() {
    let (document, table, outside) = fixture();
    let tree = TableTreeTemplate::capture(&document, table).unwrap();
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: table })
        .with_step(TransactionStep::RemoveNode { node: outside })
        .with_step(TransactionStep::InsertTableTree {
            parent: document.root(),
            index: 0,
            tree,
        })
        .apply_with_changes(&document)
        .unwrap();
    assert_eq!(applied.document().root(), document.root());
    let roots = applied
        .document()
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(roots.len(), 1);
    assert_ne!(roots[0], table);
    round_trip(&document, &applied);
}

#[test]
fn wrong_source_kind_destination_kind_and_index_fail_without_state_changes() {
    let (document, table, outside) = fixture();
    assert_eq!(
        TableTreeTemplate::capture(&document, outside).unwrap_err(),
        Error::InvalidTableStructure
    );
    let tree = TableTreeTemplate::capture(&document, table).unwrap();
    let row = document
        .node(table)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    for (parent, index) in [(row, 0), (outside, 0), (document.root(), usize::MAX)] {
        assert!(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::InsertTableTree {
                    parent,
                    index,
                    tree: tree.clone()
                })
                .apply_with_changes(&document)
                .is_err()
        );
    }
    let baseline = insert(&document, tree.clone(), 1);
    let failed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTableTree {
            parent: document.root(),
            index: 1,
            tree: tree.clone(),
        })
        .with_step(TransactionStep::RemoveNode {
            node: document.root(),
        })
        .apply_with_changes(&document);
    assert_eq!(failed.unwrap_err(), Error::InvalidTransaction);
    assert_eq!(
        insert(&document, tree, 1).document().store(),
        baseline.document().store()
    );
}

#[test]
fn template_capture_refuses_excessive_tree_or_attribute_depth() {
    for attr_depth in [false, true] {
        let mut builder = NodeStoreBuilder::new();
        let mut block = paragraph(&mut builder, "deep");
        let cell_attrs = if attr_depth {
            let mut value = AttrValue::Null;
            for _ in 0..64 {
                value = AttrValue::List(vec![value]);
            }
            attrs(&[("opaque", value)])
        } else {
            for _ in 0..128 {
                block = builder
                    .insert(
                        NodeKind::Quote,
                        NodeAttrs::empty(),
                        NodeContent::children([block]),
                    )
                    .unwrap();
            }
            NodeAttrs::empty()
        };
        let cell = builder
            .insert(
                NodeKind::TableCell,
                cell_attrs,
                NodeContent::children([block]),
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
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        assert_eq!(
            TableTreeTemplate::capture(&document, table).unwrap_err(),
            Error::TableResourceLimit
        );
    }
}

#[test]
fn stale_redo_cannot_overwrite_an_already_live_copied_subtree() {
    let (document, table, _) = fixture();
    let applied = insert(
        &document,
        TableTreeTemplate::capture(&document, table).unwrap(),
        1,
    );
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(
        undo.inverse()
            .apply_with_changes(applied.document())
            .unwrap_err(),
        Error::InvalidTransaction
    );
    assert_eq!(
        undo.inverse()
            .apply_with_changes(undo.document())
            .unwrap()
            .document()
            .store(),
        applied.document().store()
    );
}
