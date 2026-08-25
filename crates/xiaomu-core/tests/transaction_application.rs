//! P0.4 transaction application against document snapshots.

use std::collections::BTreeMap;

use xiaomu_core::Error;
use xiaomu_core::document::{
    HeadingLevel, InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::text::TextBuffer;
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn offset_at(raw: usize) -> xiaomu_core::text::TextOffset {
    const SCRATCH: &str = "00000000000000000000000000000000";
    TextBuffer::from(SCRATCH).offset_at(raw).unwrap()
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(offset_at(start), offset_at(end)).unwrap()
}

fn inline_node(
    builder: &mut NodeStoreBuilder,
    text: &str,
    marks: MarkSet,
) -> xiaomu_core::document::NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new([TextRun::new(text, marks).unwrap()]).unwrap()),
        )
        .unwrap()
}

/// `Document > [p("你好世界"), p("second")]`
fn fixture() -> (XiaomuDocument, [xiaomu_core::document::NodeId; 2]) {
    let mut builder = NodeStoreBuilder::new();
    let first = inline_node(&mut builder, "你好世界", MarkSet::empty());
    let second = inline_node(&mut builder, "second", MarkSet::empty());
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    (document, [first, second])
}

fn text_of(document: &XiaomuDocument, node: xiaomu_core::document::NodeId) -> String {
    document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

/// Returns the second paragraph of [`fixture`] documents.
fn second_of(document: &XiaomuDocument) -> xiaomu_core::document::NodeId {
    document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[1]
}

#[test]
fn replace_text_edits_and_bumps_revision() {
    let (document, [first, _]) = fixture();
    let original_revision = document.revision();

    let transaction =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::ReplaceText {
            node: first,
            range: range(3, 6),
            replacement: "，".to_owned(),
        });

    let next = transaction.apply(&document).unwrap();
    assert!(next.validate().is_ok());
    assert_eq!(next.revision().as_u64(), original_revision.as_u64() + 1);
    // range(3,6) covers exactly "好".
    assert_eq!(text_of(&next, first), "你，世界");

    // Original snapshot untouched.
    assert_eq!(text_of(&document, first), "你好世界");
}

#[test]
fn replace_text_can_delete_and_empty_a_node() {
    let (document, [first, _]) = fixture();

    let transaction =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::ReplaceText {
            node: first,
            range: range(0, 12),
            replacement: String::new(),
        });

    let next = transaction.apply(&document).unwrap();
    assert!(next.validate().is_ok());
    assert!(
        next.node(first)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn replace_text_rejects_stale_and_invalid_ranges_atomically() {
    let (document, [first, _]) = fixture();

    // Mid-code-point end boundary: 中(0..3) 好(3..6) → byte 4 splits 好.
    let bad =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::ReplaceText {
            node: first,
            range: range(0, 4),
            replacement: "x".to_owned(),
        });
    assert!(matches!(
        bad.apply(&document),
        Err(Error::InvalidTextBoundary { offset: 4 })
    ));

    // A valid step followed by an invalid one leaves no partial state.
    let mixed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceText {
            node: first,
            range: range(0, 3),
            replacement: "好".to_owned(),
        })
        .with_step(TransactionStep::ReplaceText {
            node: first,
            range: range(0, 1),
            replacement: "y".to_owned(),
        });
    assert!(mixed.apply(&document).is_err());
    assert_eq!(text_of(&document, first), "你好世界");
}

#[test]
fn insert_node_allocates_stable_ids_and_validates_structure() {
    let (document, _) = fixture();

    let transaction = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::InsertNode {
            parent: document.root(),
            index: 1,
            kind: NodeKind::Heading(HeadingLevel::new(3).unwrap()),
            attrs: NodeAttrs::empty(),
            content: NodeContent::Inline(
                InlineContent::new([TextRun::new("标题", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        })
        .with_step(TransactionStep::InsertNode {
            parent: document.root(),
            index: 2,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        });

    let next = transaction.apply(&document).unwrap();
    assert!(next.validate().is_ok());
    assert_eq!(next.node_count(), document.node_count() + 2);

    // Inserted nodes occupy deterministic positions and keep fresh identities.
    let root_children = next
        .node(next.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec();
    assert_eq!(root_children.len(), 4);
    let inserted = next.node(root_children[1]).unwrap();
    assert!(matches!(inserted.kind(), NodeKind::Heading(_)));
    assert_ne!(root_children[1], root_children[0]);
}

#[test]
fn insert_node_rejects_bad_parent_shape_and_index() {
    let (document, [first, _]) = fixture();

    let into_inline =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::InsertNode {
            parent: first,
            index: 0,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        });
    assert!(matches!(
        into_inline.apply(&document),
        Err(Error::InvalidTransaction)
    ));

    let out_of_bounds =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::InsertNode {
            parent: document.root(),
            index: 9,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        });
    assert!(matches!(
        out_of_bounds.apply(&document),
        Err(Error::InvalidTransaction)
    ));
}

#[test]
fn remove_node_removes_whole_subtree_and_keeps_validation_green() {
    let mut builder = NodeStoreBuilder::new();
    let child = inline_node(&mut builder, "child", MarkSet::empty());
    let list_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([child]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([list_item]),
        )
        .unwrap();
    let survivor = inline_node(&mut builder, "keep", MarkSet::empty());
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([list, survivor]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: list });
    let next = transaction.apply(&document).unwrap();

    assert!(next.validate().is_ok());
    assert!(next.node(list).is_none());
    assert!(next.node(list_item).is_none());
    assert!(next.node(child).is_none());
    assert!(next.node(survivor).is_some());
    assert_eq!(next.node_count(), 2); // root + survivor

    // Root removal is rejected.
    let remove_root = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: root });
    assert!(matches!(
        remove_root.apply(&next),
        Err(Error::InvalidTransaction)
    ));
}

#[test]
fn remove_node_rejects_unknown_nodes() {
    let (document, [first, _]) = fixture();

    let removal = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: first });
    let next = removal.apply(&document).unwrap();

    // A previously valid NodeId becomes unknown once its node is deleted.
    let stale_text =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::ReplaceText {
            node: first,
            range: range(0, 1),
            replacement: "x".to_owned(),
        });
    assert!(matches!(stale_text.apply(&next), Err(Error::UnknownNode)));

    let stale_remove = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: first });
    assert!(matches!(stale_remove.apply(&next), Err(Error::UnknownNode)));
}

#[test]
fn set_node_attrs_replaces_whole_set() {
    let (document, [first, _]) = fixture();

    let mut values = BTreeMap::new();
    values.insert(
        "lang".to_owned(),
        xiaomu_core::document::AttrValue::String("zh".to_owned()),
    );
    let attrs = NodeAttrs::new(values).unwrap();

    let transaction = Transaction::new(TransactionOrigin::Extension("i18n".to_owned())).with_step(
        TransactionStep::SetNodeAttrs {
            node: first,
            attrs: attrs.clone(),
        },
    );
    let next = transaction.apply(&document).unwrap();

    assert_eq!(next.node(first).unwrap().attrs(), &attrs);
    assert!(next.node(first).unwrap().attrs().get("lang").is_some());
}

#[test]
fn set_node_kind_keeps_identity_and_content() {
    let (document, [first, _]) = fixture();
    let heading = NodeKind::Heading(HeadingLevel::new(2).unwrap());

    let next = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeKind {
            node: first,
            kind: heading.clone(),
        })
        .apply(&document)
        .unwrap();

    assert_eq!(next.node(first).unwrap().kind(), &heading);
    assert_eq!(text_of(&next, first), "你好世界");
    assert_eq!(next.node_count(), document.node_count());
}

#[test]
fn set_node_kind_rejects_incompatible_shape_and_the_root() {
    let (document, [first, _]) = fixture();

    let to_quote =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeKind {
            node: first,
            kind: NodeKind::Quote,
        });
    assert_eq!(
        to_quote.apply(&document).unwrap_err(),
        Error::InvalidNodeContent
    );

    let on_root =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::SetNodeKind {
            node: document.root(),
            kind: NodeKind::Quote,
        });
    assert_eq!(
        on_root.apply(&document).unwrap_err(),
        Error::InvalidRootNode
    );

    assert_eq!(document.node(first).unwrap().kind(), &NodeKind::Paragraph);
    assert!(document.validate().is_ok());
}

#[test]
fn add_and_remove_mark_split_ranges_deterministically() {
    let (document, [first, _]) = fixture();

    let bold = Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::AddMark {
        node: first,
        range: range(3, 9),
        mark: Mark::Bold,
    });
    let marked = bold.apply(&document).unwrap();
    assert!(marked.validate().is_ok());

    let runs = marked
        .node(first)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs.len(), 3);
    assert!(!runs[0].marks().contains(MarkKind::Bold));
    assert!(runs[1].marks().contains(MarkKind::Bold));
    assert!(!runs[2].marks().contains(MarkKind::Bold));

    let unbold =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::RemoveMark {
            node: first,
            range: range(0, 12),
            mark_kind: MarkKind::Bold,
        });
    let cleared = unbold.apply(&marked).unwrap();
    let runs = cleared
        .node(first)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs.len(), 1); // normalization re-merges unmarked runs
    assert_eq!(text_of(&cleared, first), "你好世界");
}

#[test]
fn add_mark_replaces_conflicting_links_in_range() {
    let (document, [first, _]) = fixture();
    let old_link =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::AddMark {
            node: first,
            range: range(0, 6),
            mark: Mark::Link(xiaomu_core::document::LinkMark::new(
                "https://old.example",
                None,
            )),
        });
    let linked = old_link.apply(&document).unwrap();

    let new_link =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::AddMark {
            node: first,
            range: range(0, 6),
            mark: Mark::Link(xiaomu_core::document::LinkMark::new(
                "https://new.example",
                None,
            )),
        });
    let updated = new_link.apply(&linked).unwrap();
    let runs = updated
        .node(first)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(
        runs[0].marks().as_slice(),
        &[Mark::Link(xiaomu_core::document::LinkMark::new(
            "https://new.example",
            None
        ))]
    );
}

#[test]
fn transactions_carry_origin_and_metadata() {
    let mut transaction = Transaction::new(TransactionOrigin::Extension("demo".to_owned()));
    transaction.set_metadata("group", "typing").unwrap();
    transaction.set_metadata("source", "ime-commit").unwrap();

    assert!(matches!(
        transaction.origin(),
        TransactionOrigin::Extension(name) if name == "demo"
    ));
    let metadata: Vec<_> = transaction.metadata().collect();
    assert_eq!(metadata, [("group", "typing"), ("source", "ime-commit")]);

    assert!(transaction.set_metadata("", "x").is_err());
}

#[test]
fn empty_transaction_still_bumps_revision_and_revalidates() {
    let (document, _) = fixture();
    let next = Transaction::new(TransactionOrigin::System)
        .apply(&document)
        .unwrap();

    assert!(next.validate().is_ok());
    assert_eq!(next.revision().as_u64(), document.revision().as_u64() + 1);
    assert_eq!(next.node_count(), document.node_count());
}

#[test]
fn split_node_moves_tail_text_into_a_new_sibling() {
    let (document, [first, _]) = fixture();

    let transaction =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SplitNode {
            node: first,
            at: offset_at(6),
        });
    let applied = transaction.apply_with_changes(&document).unwrap();
    let next = applied.document();
    assert!(next.validate().is_ok());

    // The original node keeps the head text; a sibling with the same kind
    // and attributes follows it in the parent's child list.
    assert_eq!(text_of(next, first), "你好");
    let children = next
        .node(next.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0], first);

    let tail = children[1];
    assert_eq!(text_of(next, tail), "世界");
    assert_eq!(next.node(tail).unwrap().kind(), &NodeKind::Paragraph);
}

#[test]
fn split_node_at_run_boundary_keeps_runs_whole() {
    // bold("你好") + plain("tail"): splitting exactly at byte 6 must leave
    // each run on one side, not inherit marks across the boundary.
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new("你好", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
                    TextRun::new("tail", MarkSet::empty()).unwrap(),
                ])
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let transaction =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SplitNode {
            node: paragraph,
            at: offset_at(6),
        });
    let applied = transaction.apply_with_changes(&document).unwrap();
    let next = applied.document();

    let bold_flags = |doc: &XiaomuDocument, node| -> Vec<(String, bool)> {
        doc.node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()
            .iter()
            .map(|run| {
                (
                    run.text().as_str().to_owned(),
                    run.marks().as_slice().contains(&Mark::Bold),
                )
            })
            .collect()
    };

    let children = next
        .node(next.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(
        bold_flags(next, children[0]),
        vec![("你好".to_owned(), true)]
    );
    assert_eq!(
        bold_flags(next, children[1]),
        vec![("tail".to_owned(), false)]
    );

    // Splitting inside the bold run gives both halves of THAT RUN bold
    // marks; other runs keep their own marks.
    let inner =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SplitNode {
            node: paragraph,
            at: offset_at(3),
        });
    let inner = inner.apply(&document).unwrap();
    let children = inner
        .node(inner.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(text_of(&inner, children[0]), "你");
    assert_eq!(text_of(&inner, children[1]), "好tail");
    let tail_runs = bold_flags(&inner, children[1]);
    assert_eq!(
        tail_runs,
        vec![("好".to_owned(), true), ("tail".to_owned(), false)]
    );
}

#[test]
fn split_node_rejects_invalid_targets_and_offsets() {
    let (document, [first, _]) = fixture();

    // Offsets must be UTF-8 boundaries of the concatenated text.
    let mid_scalar =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SplitNode {
            node: first,
            at: offset_at(1),
        });
    assert!(matches!(
        mid_scalar.apply(&document),
        Err(Error::InvalidTextBoundary { .. })
    ));

    // Out-of-bounds offsets fail too.
    let out_of_bounds_offset = xiaomu_core::text::TextBuffer::from("0".repeat(100))
        .offset_at(99)
        .unwrap();
    let out_of_bounds =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SplitNode {
            node: first,
            at: out_of_bounds_offset,
        });
    assert_eq!(
        out_of_bounds.apply(&document).unwrap_err(),
        Error::TextOutOfBounds {
            offset: 99,
            len: 12
        }
    );

    // Unknown nodes fail atomically: a previously valid NodeId becomes
    // unknown once its node is deleted.
    let removal =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::RemoveNode {
            node: second_of(&document),
        });
    let next = removal.apply(&document).unwrap();
    let unknown =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::SplitNode {
            node: second_of(&document),
            at: offset_at(0),
        });
    assert_eq!(unknown.apply(&next).unwrap_err(), Error::UnknownNode);

    // Structural containers are not inline-bearing, so they cannot split.
    let mut builder = NodeStoreBuilder::new();
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([quote]),
        )
        .unwrap();
    let container_document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let on_container =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SplitNode {
            node: quote,
            at: offset_at(0),
        });
    assert_eq!(
        on_container.apply(&container_document).unwrap_err(),
        Error::InvalidTransaction
    );

    // None of the failures changed anything.
    assert!(document.validate().is_ok());
}

#[test]
fn join_nodes_merges_adjacent_siblings_into_the_first() {
    let (document, [first, second]) = fixture();

    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::JoinNodes { first, second });
    let applied = transaction.apply_with_changes(&document).unwrap();
    let next = applied.document();

    assert!(next.validate().is_ok());
    assert!(next.node(first).is_some());
    assert!(next.node(second).is_none());
    assert_eq!(text_of(next, first), "你好世界second");

    let children = next
        .node(next.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0], first);

    // Mapping moves positions of the absorbed node into the survivor.
    use xiaomu_core::mapping::{MapBias, MappedPosition};
    use xiaomu_core::selection::{CursorAffinity, NodeSelection, TextPoint};
    let mapped_point = TextPoint::new(second, offset_at(2), CursorAffinity::Before);
    assert_eq!(
        applied
            .changes()
            .map_text_point(mapped_point, MapBias::Start),
        MappedPosition::Mapped(TextPoint::new(first, offset_at(14), CursorAffinity::Before))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_selection(NodeSelection::new(second)),
        MappedPosition::Deleted
    );
}

#[test]
fn join_nodes_requires_adjacent_inline_siblings() {
    let (document, [first, second]) = fixture();

    // Reversed order is rejected: `second` must immediately follow `first`.
    let reversed =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::JoinNodes {
            first: second,
            second: first,
        });
    assert_eq!(
        reversed.apply(&document).unwrap_err(),
        Error::InvalidTransaction
    );

    // Non-siblings are rejected even when both identities exist.
    let mut builder = NodeStoreBuilder::new();
    let first_copy = inline_node(&mut builder, "one", MarkSet::empty());
    let second_copy = inline_node(&mut builder, "two", MarkSet::empty());
    let nested = inline_node(&mut builder, "nested", MarkSet::empty());
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([nested]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first_copy, second_copy, quote]),
        )
        .unwrap();
    let sibling_document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let cross_parent =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::JoinNodes {
            first: first_copy,
            second: nested,
        });
    assert_eq!(
        cross_parent.apply(&sibling_document).unwrap_err(),
        Error::InvalidTransaction
    );

    assert!(document.validate().is_ok());
}
