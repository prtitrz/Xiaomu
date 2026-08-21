use std::collections::BTreeMap;

use xiaomu_core::{
    Error,
    document::{
        AttrValue, HeadingLevel, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    },
};

#[test]
fn node_attrs_preserve_unknown_values_in_deterministic_order() {
    let mut values = BTreeMap::new();
    values.insert("zeta".to_owned(), AttrValue::Integer(7));
    values.insert("alpha".to_owned(), AttrValue::String("kept".to_owned()));
    let attrs = NodeAttrs::new(values).unwrap();

    let keys: Vec<_> = attrs.iter().map(|(key, _)| key).collect();
    assert_eq!(keys, vec!["alpha", "zeta"]);
    assert_eq!(
        attrs.get("alpha"),
        Some(&AttrValue::String("kept".to_owned()))
    );
}

#[test]
fn node_attrs_reject_empty_keys() {
    let mut values = BTreeMap::new();
    values.insert("   ".to_owned(), AttrValue::Bool(true));

    assert_eq!(NodeAttrs::new(values), Err(Error::InvalidNodeAttrKey));
}

#[test]
fn inline_content_merges_adjacent_runs_with_identical_marks() {
    let marks = MarkSet::new([Mark::Bold]).unwrap();
    let first = TextRun::new("晓", marks.clone()).unwrap();
    let second = TextRun::new("木", marks).unwrap();
    let third = TextRun::new("🙂", MarkSet::empty()).unwrap();

    let inline = InlineContent::new([first, second, third]).unwrap();

    assert_eq!(inline.runs().len(), 2);
    assert_eq!(inline.runs()[0].text().as_str(), "晓木");
    assert_eq!(inline.runs()[1].text().as_str(), "🙂");
    assert_eq!(inline.len_bytes(), "晓木🙂".len());
}

#[test]
fn empty_paragraph_is_valid_without_persisting_an_empty_text_run() {
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

    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    assert_eq!(document.node_count(), 2);
    assert!(document.validate().is_ok());
}

#[test]
fn valid_tree_preserves_node_semantics_and_root_identity() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("hello 晓木", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap();
    let heading = builder
        .insert(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([heading, paragraph]),
        )
        .unwrap();

    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    assert_eq!(document.root(), root);
    assert_eq!(document.node_count(), 3);
    assert!(matches!(
        document.node(heading).unwrap().kind(),
        NodeKind::Heading(_)
    ));
}

#[test]
fn node_kind_rejects_incompatible_content_shape() {
    let mut builder = NodeStoreBuilder::new();

    assert_eq!(
        builder.insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::children([]),
        ),
        Err(Error::InvalidNodeContent)
    );
}

#[test]
fn list_rejects_non_list_item_children() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();

    assert_eq!(
        builder.insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        ),
        Err(Error::InvalidChildKind)
    );
}

#[test]
fn duplicate_child_reference_is_rejected_during_safe_building() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();

    assert_eq!(
        builder.insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph, paragraph]),
        ),
        Err(Error::DuplicateChildReference)
    );
}

#[test]
fn root_must_be_a_document_node() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();

    assert_eq!(
        XiaomuDocument::new(paragraph, builder.finish()).unwrap_err(),
        Error::InvalidRootNode
    );
}

#[test]
fn unreachable_nodes_are_rejected() {
    let mut builder = NodeStoreBuilder::new();
    let _orphan = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let reachable = builder
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
            NodeContent::children([reachable]),
        )
        .unwrap();

    assert_eq!(
        XiaomuDocument::new(root, builder.finish()).unwrap_err(),
        Error::UnreachableNode
    );
}

#[test]
fn a_node_cannot_have_multiple_parents() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let first_quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let second_quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first_quote, second_quote]),
        )
        .unwrap();

    assert_eq!(
        XiaomuDocument::new(root, builder.finish()).unwrap_err(),
        Error::MultipleNodeParents
    );
}
