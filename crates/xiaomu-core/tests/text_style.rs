//! Text styling is an exact canonical value, not a renderer-side annotation.

use std::collections::HashSet;
use xiaomu_core::Error;
use xiaomu_core::document::{
    InlineContent, LinkMark, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeKind,
    NodeStoreBuilder, StringAttribute, TextRun, TextStyleAttributes, TextStyleMark, XiaomuDocument,
};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn style(values: [StringAttribute; 3]) -> TextStyleMark {
    let [color, family, size] = values;
    TextStyleMark::from_attributes(
        TextStyleAttributes::default()
            .with_color(color)
            .with_font_family(family)
            .with_font_size(size),
    )
}

#[test]
fn independent_states_are_exact_hashable_and_never_cleaned_up() {
    let states = [
        StringAttribute::Missing,
        StringAttribute::Null,
        StringAttribute::Value(String::new()),
        StringAttribute::Value("未解析🙂; 'quoted' calc(100% + 2px)".into()),
    ];
    let mut distinct = HashSet::new();
    for mut code in 0..4usize.pow(3) {
        let values: [_; 3] = std::array::from_fn(|_| {
            let value = states[code % 4].clone();
            code /= 4;
            value
        });
        let mark = style(values.clone());
        assert_eq!(
            [
                mark.attributes().color(),
                mark.attributes().font_family(),
                mark.attributes().font_size()
            ],
            values.each_ref()
        );
        let marks =
            MarkSet::new([Mark::TextStyle(mark.clone()), Mark::TextStyle(mark.clone())]).unwrap();
        assert_eq!(marks.as_slice(), &[Mark::TextStyle(mark.clone())]);
        assert!(marks.contains(MarkKind::TextStyle));
        assert!(!marks.is_empty());
        distinct.insert(mark);
    }
    assert_eq!(distinct.len(), 64);
}

#[test]
fn conflicting_fields_reject_but_other_mark_kinds_coexist() {
    let base = style(std::array::from_fn(|_| StringAttribute::Missing));
    for index in 0..3 {
        for value in [StringAttribute::Null, StringAttribute::Value(String::new())] {
            let mut values = std::array::from_fn(|_| StringAttribute::Missing);
            values[index] = value;
            assert_eq!(
                MarkSet::new([
                    Mark::TextStyle(base.clone()),
                    Mark::TextStyle(style(values))
                ]),
                Err(Error::InvalidMarkSet)
            );
        }
    }
    let marks = MarkSet::new([
        Mark::TextStyle(base),
        Mark::Code,
        Mark::Bold,
        Mark::Link(LinkMark::new("x", None)),
    ])
    .unwrap();
    assert_eq!(
        marks.len(),
        4,
        "Core does not implement host mark exclusions"
    );
}

#[test]
fn adjacent_runs_merge_only_when_all_text_style_states_match() {
    let missing = Mark::TextStyle(style(std::array::from_fn(|_| StringAttribute::Missing)));
    let null = Mark::TextStyle(style(std::array::from_fn(|_| StringAttribute::Null)));
    let inline = InlineContent::new([
        TextRun::new("a", MarkSet::new([missing.clone()]).unwrap()).unwrap(),
        TextRun::new("b", MarkSet::new([missing]).unwrap()).unwrap(),
        TextRun::new("c", MarkSet::new([null]).unwrap()).unwrap(),
        TextRun::new("d", MarkSet::empty()).unwrap(),
    ])
    .unwrap();
    assert_eq!(inline.runs().len(), 3);
    assert_eq!(inline.runs()[0].text().as_str(), "ab");
}

#[test]
fn exact_replacement_and_removal_have_lossless_inverses() {
    let old = Mark::TextStyle(style([
        StringAttribute::Null,
        "Noto Sans SC, serif".into(),
        "".into(),
    ]));
    let new = Mark::TextStyle(style([
        "not-css".into(),
        StringAttribute::Missing,
        "24px".into(),
    ]));
    let inline =
        InlineContent::new([
            TextRun::new("中🙂Z", MarkSet::new([Mark::Bold, old]).unwrap()).unwrap(),
        ])
        .unwrap();
    let range = TextRange::new(inline.offset_at(0).unwrap(), inline.offset_at(7).unwrap()).unwrap();
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
    let original = XiaomuDocument::new(root, builder.finish()).unwrap();
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::AddMark {
            node,
            range,
            mark: new.clone(),
        })
        .apply_with_changes(&original)
        .unwrap();
    let runs = applied
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs[0].marks().as_slice(), &[Mark::Bold, new]);
    assert_eq!(runs[1].text().as_str(), "Z");
    assert_eq!(
        applied.inverse().apply(applied.document()).unwrap().store(),
        original.store()
    );
    let removed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveMark {
            node,
            range,
            mark_kind: MarkKind::TextStyle,
        })
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(
        removed
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .marks()
            .as_slice(),
        &[Mark::Bold]
    );
    assert_eq!(
        removed.inverse().apply(removed.document()).unwrap().store(),
        applied.document().store()
    );
}
