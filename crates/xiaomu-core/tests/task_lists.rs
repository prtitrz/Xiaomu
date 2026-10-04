//! Typed task containers preserve exact attributes and generic tree semantics.

use xiaomu_core::Error;
use xiaomu_core::document::{
    AtomKind, AttrValue, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark,
    MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition};
use xiaomu_core::selection::{InlinePoint, NodeGap, NodeSelection};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn attrs(checked: Option<AttrValue>) -> NodeAttrs {
    let mut values = std::collections::BTreeMap::from([(
        "extension".into(),
        AttrValue::Object([("nested".into(), AttrValue::List(vec![AttrValue::Null]))].into()),
    )]);
    if let Some(checked) = checked {
        values.insert("checked".into(), checked);
    }
    NodeAttrs::new(values).unwrap()
}

fn text(builder: &mut NodeStoreBuilder, kind: NodeKind, value: &str) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(value, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn container(builder: &mut NodeStoreBuilder, kind: NodeKind, children: Vec<NodeId>) -> NodeId {
    builder
        .insert(kind, NodeAttrs::empty(), NodeContent::children(children))
        .unwrap()
}

fn simple(checked: Option<AttrValue>) -> (XiaomuDocument, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = text(&mut builder, NodeKind::Paragraph, "task 中🙂");
    let item = builder
        .insert(
            NodeKind::TaskItem,
            attrs(checked),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let list = container(&mut builder, NodeKind::TaskList, vec![item]);
    let root = container(&mut builder, NodeKind::Document, vec![list]);
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        list,
        item,
        paragraph,
    )
}

#[test]
fn checked_states_are_exact_and_unknown_core_attrs_are_preserved() {
    for checked in [
        None,
        Some(AttrValue::Null),
        Some(AttrValue::Bool(false)),
        Some(AttrValue::Bool(true)),
    ] {
        let (document, _, item, _) = simple(checked.clone());
        let snapshot = document.store().clone();
        assert_eq!(
            document.node(item).unwrap().attrs(),
            &attrs(checked.clone())
        );
        assert_eq!(
            document.node(item).unwrap().attrs().get("checked"),
            checked.as_ref()
        );
        // Reading and validating must not materialize a display default.
        document.validate().unwrap();
        assert_eq!(document.store(), &snapshot);
        assert_eq!(document.version().as_u32(), 1);
    }
}

#[test]
fn checked_rejects_non_boolean_values_during_construction_and_mutation() {
    let (document, _, item, _) = simple(None);
    for invalid in [
        AttrValue::Integer(0),
        AttrValue::Integer(1),
        AttrValue::String("false".into()),
        AttrValue::List(vec![]),
        AttrValue::Object(Default::default()),
    ] {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = text(&mut builder, NodeKind::Paragraph, "task");
        assert_eq!(
            builder.insert(
                NodeKind::TaskItem,
                attrs(Some(invalid.clone())),
                NodeContent::children([paragraph])
            ),
            Err(Error::InvalidTaskItemChecked)
        );
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::SetNodeAttrs {
                node: item,
                attrs: attrs(Some(invalid)),
            },
        );
        assert_eq!(
            transaction.apply(&document).unwrap_err(),
            Error::InvalidTaskItemChecked
        );
        assert_eq!(document.node(item).unwrap().attrs().get("checked"), None);
    }
    // This semantic constraint belongs to TaskItem, not the generic attr bag.
    let mut builder = NodeStoreBuilder::new();
    let list = builder
        .insert(
            NodeKind::TaskList,
            attrs(Some(AttrValue::String("extension".into()))),
            NodeContent::children([]),
        )
        .unwrap();
    let root = container(&mut builder, NodeKind::Document, vec![list]);
    XiaomuDocument::new(root, builder.finish()).unwrap();
}

#[test]
fn all_checked_transitions_have_exact_inverse_and_identity_mapping() {
    let states = [
        None,
        Some(AttrValue::Null),
        Some(AttrValue::Bool(false)),
        Some(AttrValue::Bool(true)),
    ];
    for before in &states {
        for after in &states {
            let (document, _, item, paragraph) = simple(before.clone());
            let applied = Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::SetNodeAttrs {
                    node: item,
                    attrs: attrs(after.clone()),
                })
                .apply_with_changes(&document)
                .unwrap();
            assert_eq!(
                applied.document().node(item).unwrap().attrs(),
                &attrs(after.clone())
            );
            let point = InlinePoint::at_start_of(paragraph);
            assert_eq!(
                applied.changes().map_inline_point(point, MapBias::End),
                MappedPosition::Mapped(point)
            );
            assert_eq!(
                applied
                    .changes()
                    .map_node_gap(NodeGap::new(item, 1), MapBias::Start),
                MappedPosition::Mapped(NodeGap::new(item, 1))
            );
            assert_eq!(
                applied.inverse().apply(applied.document()).unwrap().store(),
                document.store()
            );
        }
    }
}

#[test]
fn core_allows_empty_or_nonparagraph_task_items_but_rejects_ordinary_items() {
    for first_kind in [
        None,
        Some(NodeKind::CodeBlock),
        Some(NodeKind::Heading(HeadingLevel::new(1).unwrap())),
    ] {
        let mut builder = NodeStoreBuilder::new();
        let children = first_kind
            .map(|kind| text(&mut builder, kind, "wrong first block"))
            .into_iter()
            .collect();
        let item = container(&mut builder, NodeKind::TaskItem, children);
        let list = container(&mut builder, NodeKind::TaskList, vec![item]);
        let root = container(&mut builder, NodeKind::Document, vec![list]);
        XiaomuDocument::new(root, builder.finish())
            .unwrap()
            .validate()
            .unwrap();
    }
    let mut builder = NodeStoreBuilder::new();
    let paragraph = text(&mut builder, NodeKind::Paragraph, "text");
    let ordinary_item = container(&mut builder, NodeKind::ListItem, vec![paragraph]);
    assert_eq!(
        builder.insert(
            NodeKind::TaskList,
            NodeAttrs::empty(),
            NodeContent::children([ordinary_item])
        ),
        Err(Error::InvalidChildKind)
    );
    let (document, list, item, _) = simple(None);
    for step in [
        TransactionStep::SetNodeKind {
            node: item,
            kind: NodeKind::ListItem,
        },
        TransactionStep::SetNodeKind {
            node: list,
            kind: NodeKind::BulletList,
        },
    ] {
        assert!(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(step)
                .apply(&document)
                .is_err()
        );
        document.validate().unwrap();
    }
}

#[test]
fn task_items_are_not_blocks_or_ordinary_list_items() {
    for parent_kind in [
        NodeKind::Document,
        NodeKind::Quote,
        NodeKind::ListItem,
        NodeKind::TaskItem,
        NodeKind::BulletList,
        NodeKind::OrderedList,
        NodeKind::TableCell,
    ] {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = text(&mut builder, NodeKind::Paragraph, "child");
        let item = container(&mut builder, NodeKind::TaskItem, vec![paragraph]);
        assert_eq!(
            builder.insert(
                parent_kind,
                NodeAttrs::empty(),
                NodeContent::children([item])
            ),
            Err(Error::InvalidChildKind)
        );
    }
    for kind in [NodeKind::TaskList, NodeKind::TaskItem] {
        for content in [NodeContent::Atomic, NodeContent::empty_inline()] {
            assert_eq!(
                NodeStoreBuilder::new().insert(kind.clone(), NodeAttrs::empty(), content),
                Err(Error::InvalidNodeContent)
            );
        }
    }
}

#[test]
fn mixed_task_subtrees_include_code_images_and_hard_breaks_with_exact_remove_restore() {
    let mut builder = NodeStoreBuilder::new();
    let marks = MarkSet::new([Mark::Bold]).unwrap();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break().with_marks(marks)),
        )
        .unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            attrs(None),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("task🙂", MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let code = text(&mut builder, NodeKind::CodeBlock, "let x = 1;\n中");
    let image = builder
        .insert(NodeKind::Image, attrs(None), NodeContent::Atomic)
        .unwrap();
    let ordinary_text = text(&mut builder, NodeKind::Paragraph, "ordinary");
    let ordinary_item = container(&mut builder, NodeKind::ListItem, vec![ordinary_text]);
    let ordinary_list = container(&mut builder, NodeKind::OrderedList, vec![ordinary_item]);
    let nested_text = text(&mut builder, NodeKind::Paragraph, "nested task");
    let nested_item = builder
        .insert(
            NodeKind::TaskItem,
            attrs(Some(AttrValue::Null)),
            NodeContent::children([nested_text]),
        )
        .unwrap();
    let nested_list = container(&mut builder, NodeKind::TaskList, vec![nested_item]);
    let quote = container(&mut builder, NodeKind::Quote, vec![nested_list]);
    let task = builder
        .insert(
            NodeKind::TaskItem,
            attrs(Some(AttrValue::Bool(true))),
            NodeContent::children([paragraph, code, image, ordinary_list, quote]),
        )
        .unwrap();
    let list = container(&mut builder, NodeKind::TaskList, vec![task]);
    let outer_text = text(&mut builder, NodeKind::Paragraph, "outer ordinary");
    let outer_item = container(&mut builder, NodeKind::ListItem, vec![outer_text, list]);
    let outer_list = container(&mut builder, NodeKind::BulletList, vec![outer_item]);
    let tail = text(&mut builder, NodeKind::Paragraph, "outside");
    let root = container(&mut builder, NodeKind::Document, vec![outer_list, tail]);
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let removed = [
        list,
        task,
        paragraph,
        atom,
        code,
        image,
        ordinary_list,
        ordinary_item,
        ordinary_text,
        quote,
        nested_list,
        nested_item,
        nested_text,
    ];
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: list })
        .apply_with_changes(&document)
        .unwrap();
    for id in removed {
        assert!(applied.document().node(id).is_none());
        assert_eq!(
            applied.changes().map_node_selection(NodeSelection::new(id)),
            MappedPosition::Deleted
        );
    }
    assert_eq!(
        applied
            .changes()
            .map_inline_point(InlinePoint::at_start_of(paragraph), MapBias::End),
        MappedPosition::Deleted
    );
    assert_eq!(
        applied
            .changes()
            .map_node_gap(NodeGap::new(outer_item, 2), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(outer_item, 1))
    );
    let tail_point = InlinePoint::at_start_of(tail);
    assert_eq!(
        applied
            .changes()
            .map_inline_point(tail_point, MapBias::Start),
        MappedPosition::Mapped(tail_point)
    );
    let restored = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(restored.document().store(), document.store());
    assert_eq!(
        restored
            .changes()
            .map_node_gap(NodeGap::new(outer_item, 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(outer_item, 2))
    );
    assert_eq!(
        restored
            .inverse()
            .apply(restored.document())
            .unwrap()
            .store(),
        applied.document().store()
    );
}

#[test]
fn generic_transaction_staging_builds_and_lifts_task_blocks_without_host_schema_rules() {
    let (document, _, task, paragraph) = simple(None);
    let removed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: paragraph })
        .apply_with_changes(&document)
        .unwrap();
    assert!(
        removed
            .document()
            .node(task)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        removed.inverse().apply(removed.document()).unwrap().store(),
        document.store()
    );
    let inserted = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertNode {
            parent: task,
            index: 0,
            kind: NodeKind::CodeBlock,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        })
        .apply_with_changes(removed.document())
        .unwrap();
    let leading = inserted
        .document()
        .node(task)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    assert_eq!(
        inserted.document().node(leading).unwrap().kind(),
        &NodeKind::CodeBlock
    );
    assert_eq!(
        inserted
            .inverse()
            .apply(inserted.document())
            .unwrap()
            .store(),
        removed.document().store()
    );

    let mut builder = NodeStoreBuilder::new();
    let root = container(&mut builder, NodeKind::Document, vec![]);
    let empty = XiaomuDocument::new(root, builder.finish()).unwrap();
    let list_stage = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertNode {
            parent: root,
            index: 0,
            kind: NodeKind::TaskList,
            attrs: NodeAttrs::empty(),
            content: NodeContent::children([]),
        })
        .apply_with_changes(&empty)
        .unwrap();
    let list = list_stage
        .document()
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let item_stage = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertNode {
            parent: list,
            index: 0,
            kind: NodeKind::TaskItem,
            attrs: attrs(Some(AttrValue::Null)),
            content: NodeContent::children([]),
        })
        .apply_with_changes(list_stage.document())
        .unwrap();
    let item = item_stage
        .document()
        .node(list)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let paragraph_stage = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertNode {
            parent: item,
            index: 0,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        })
        .apply_with_changes(item_stage.document())
        .unwrap();
    paragraph_stage.document().validate().unwrap();
    assert_eq!(
        paragraph_stage.document().node(item).unwrap().attrs(),
        &attrs(Some(AttrValue::Null))
    );
    let undo_paragraph = paragraph_stage
        .inverse()
        .apply(paragraph_stage.document())
        .unwrap();
    let undo_item = item_stage.inverse().apply(&undo_paragraph).unwrap();
    let undo_list = list_stage.inverse().apply(&undo_item).unwrap();
    assert_eq!(undo_list.store(), empty.store());
}
