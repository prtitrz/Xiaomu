//! Exact link attributes remain ordinary canonical mark values.

use xiaomu_core::Error;
use xiaomu_core::document::{
    InlineContent, LinkAttributes, LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind,
    NodeStoreBuilder, StringAttribute, TextRun, XiaomuDocument,
};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn attributes(values: &[StringAttribute; 5]) -> LinkAttributes {
    LinkAttributes::default()
        .with_href(values[0].clone())
        .with_target(values[1].clone())
        .with_rel(values[2].clone())
        .with_class(values[3].clone())
        .with_title(values[4].clone())
}

#[test]
fn all_five_fields_independently_preserve_missing_null_and_unicode_strings() {
    let states = [
        StringAttribute::Missing,
        StringAttribute::Null,
        StringAttribute::Value("链接🙂\n\"\\".into()),
    ];
    for mut permutation in 0..3usize.pow(5) {
        let values = std::array::from_fn(|_| {
            let value = states[permutation % 3].clone();
            permutation /= 3;
            value
        });
        let attrs = attributes(&values);
        let link = LinkMark::from_attributes(attrs.clone());
        assert_eq!(link.attributes(), &attrs);
        assert_eq!(
            [
                attrs.href(),
                attrs.target(),
                attrs.rel(),
                attrs.class(),
                attrs.title()
            ],
            values.each_ref()
        );
        assert_eq!(link.href(), values[0].as_str());
        assert_eq!(link.title(), values[4].as_str());
        assert_eq!(
            MarkSet::new([Mark::Link(link.clone()), Mark::Link(link.clone())])
                .unwrap()
                .as_slice(),
            &[Mark::Link(link)]
        );
    }
}

#[test]
fn empty_missing_and_null_are_distinct_and_do_not_invent_a_destination() {
    let missing = LinkMark::from_attributes(LinkAttributes::default());
    let null =
        LinkMark::from_attributes(LinkAttributes::default().with_href(StringAttribute::Null));
    let empty = LinkMark::new("", None);
    assert_ne!(missing, null);
    assert_ne!(missing, empty);
    assert_ne!(null, empty);
    assert_eq!(missing.href(), None);
    assert_eq!(null.href(), None);
    assert_eq!(empty.href(), Some(""));
    assert_eq!(missing.classic_parts(), None);
    assert_eq!(null.classic_parts(), None);
    assert_eq!(empty.classic_parts(), Some(("", None)));
    assert!(StringAttribute::default().is_missing());
    assert!(StringAttribute::Null.is_null());
}

#[test]
fn classic_constructor_preserves_old_title_semantics_and_lossless_projection() {
    for title in [None, Some("".into()), Some("标题🙂".into())] {
        let link = LinkMark::new("scheme:目的", title.clone());
        assert_eq!(
            link.classic_parts(),
            Some(("scheme:目的", title.as_deref()))
        );
        assert_eq!(
            link.attributes().title(),
            &title.map_or(StringAttribute::Missing, StringAttribute::Value)
        );
        assert!(link.attributes().target().is_missing());
        assert!(link.attributes().rel().is_missing());
        assert!(link.attributes().class().is_missing());
    }
    for index in 1..5 {
        for state in [StringAttribute::Null, StringAttribute::Value("".into())] {
            let mut values = std::array::from_fn(|_| StringAttribute::Missing);
            values[0] = "https://example.test".into();
            values[index] = state.clone();
            let link = LinkMark::from_attributes(attributes(&values));
            assert_eq!(
                link.classic_parts().is_some(),
                index == 4 && !state.is_null()
            );
        }
    }
}

#[test]
fn conflicting_link_attributes_are_rejected_even_with_the_same_href() {
    let base = LinkAttributes::default().with_href("https://example.test".into());
    let first = Mark::Link(LinkMark::from_attributes(base.clone()));
    for attrs in [
        base.clone().with_target(StringAttribute::Null),
        base.clone().with_rel("".into()),
        base.clone().with_class("ref".into()),
        base.with_title(StringAttribute::Null),
    ] {
        assert_eq!(
            MarkSet::new([first.clone(), Mark::Link(LinkMark::from_attributes(attrs))]),
            Err(Error::InvalidMarkSet)
        );
    }
}

#[test]
fn add_mark_replaces_all_link_attributes_and_inverse_restores_them_exactly() {
    let old = LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href(StringAttribute::Null)
            .with_target("_blank".into())
            .with_title(StringAttribute::Null),
    );
    let next = LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href("local:链接".into())
            .with_rel(StringAttribute::Null)
            .with_class("".into())
            .with_title("标题".into()),
    );
    let inline =
        InlineContent::new([
            TextRun::new("中🙂", MarkSet::new([Mark::Link(old)]).unwrap()).unwrap(),
        ])
        .unwrap();
    let range = TextRange::new(
        inline.offset_at(0).unwrap(),
        inline.offset_at("中🙂".len()).unwrap(),
    )
    .unwrap();
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::AddMark {
            node,
            range,
            mark: Mark::Link(next.clone()),
        })
        .apply_with_changes(&document)
        .unwrap();
    assert_eq!(
        applied
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .marks()
            .as_slice(),
        &[Mark::Link(next)]
    );
    let undone = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undone.document().store(), document.store());
    let redone = undone.inverse().apply(undone.document()).unwrap();
    assert_eq!(redone.store(), applied.document().store());
}
