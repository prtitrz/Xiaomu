//! Complete canonical document fixture, including every current kind.
use std::collections::BTreeMap;
use xiaomu_core::document::{
    AtomKind, AttrValue, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent,
    LinkAttributes, LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, StringAttribute, TextRun, TextStyleAttributes, TextStyleMark, XiaomuDocument,
};

fn marks() -> MarkSet {
    MarkSet::new([
        Mark::Bold,
        Mark::Italic,
        Mark::Code,
        Mark::Underline,
        Mark::Strike,
        Mark::Link(LinkMark::from_attributes(
            LinkAttributes::default()
                .with_href(StringAttribute::Null)
                .with_target(StringAttribute::Value(String::new())),
        )),
        Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default()
                .with_color(StringAttribute::Null)
                .with_font_size(StringAttribute::Value("19.5px".into())),
        )),
    ])
    .unwrap()
}

pub(super) fn attrs(entries: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        entries
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone()))
            .collect(),
    )
    .unwrap()
}

pub(super) fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
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

pub(super) fn fixture() -> (XiaomuDocument, NodeId, NodeId) {
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
    let mut blocks = vec![table, outside];
    for kind in [
        NodeKind::Heading(HeadingLevel::new(3).unwrap()),
        NodeKind::CodeBlock,
    ] {
        blocks.push(
            builder
                .insert(
                    kind,
                    attrs(&[("raw", AttrValue::Null)]),
                    NodeContent::Inline(
                        InlineContent::new([TextRun::new("literal\n中", marks()).unwrap()])
                            .unwrap(),
                    ),
                )
                .unwrap(),
        );
    }
    blocks.push(
        builder
            .insert(
                NodeKind::HorizontalRule,
                NodeAttrs::empty(),
                NodeContent::Atomic,
            )
            .unwrap(),
    );
    for kind in [NodeKind::BulletList, NodeKind::OrderedList] {
        let p = paragraph(&mut builder, "list");
        let item = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([p]),
            )
            .unwrap();
        blocks.push(
            builder
                .insert(
                    kind,
                    attrs(&[("start", AttrValue::Integer(3))]),
                    NodeContent::children([item]),
                )
                .unwrap(),
        );
    }
    let mut tasks = Vec::new();
    for checked in [
        None,
        Some(AttrValue::Null),
        Some(AttrValue::Bool(false)),
        Some(AttrValue::Bool(true)),
    ] {
        let p = paragraph(&mut builder, "task");
        let attrs = checked.map_or_else(NodeAttrs::empty, |value| attrs(&[("checked", value)]));
        tasks.push(
            builder
                .insert(NodeKind::TaskItem, attrs, NodeContent::children([p]))
                .unwrap(),
        );
    }
    blocks.push(
        builder
            .insert(
                NodeKind::TaskList,
                NodeAttrs::empty(),
                NodeContent::children(tasks),
            )
            .unwrap(),
    );
    let p = paragraph(&mut builder, "quote");
    blocks.push(
        builder
            .insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([p]),
            )
            .unwrap(),
    );
    let p = paragraph(&mut builder, "custom child");
    for content in [
        NodeContent::children([p]),
        NodeContent::empty_inline(),
        NodeContent::Atomic,
        NodeContent::InlineAtom(InlineAtomContent::new("custom payload").unwrap()),
    ] {
        blocks.push(
            builder
                .insert(
                    NodeKind::custom("custom:block").unwrap(),
                    NodeAttrs::empty(),
                    content,
                )
                .unwrap(),
        );
    }
    let root = builder
        .insert(
            NodeKind::Document,
            attrs(&[
                ("root-null", AttrValue::Null),
                ("root-empty", AttrValue::String(String::new())),
            ]),
            NodeContent::children(blocks),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        outside,
    )
}
