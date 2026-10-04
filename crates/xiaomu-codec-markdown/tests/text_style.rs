//! Baseline Markdown refuses every TextStyle presence and attribute state.

use xiaomu_codec_markdown::{MarkdownCodecError, to_markdown};
use xiaomu_core::document::{
    HeadingLevel, InlineContent, LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, StringAttribute, TextRun, TextStyleAttributes, TextStyleMark, XiaomuDocument,
};

fn attribute_states(value: &str) -> [StringAttribute; 4] {
    [
        StringAttribute::Missing,
        StringAttribute::Null,
        StringAttribute::Value(String::new()),
        StringAttribute::Value(value.to_owned()),
    ]
}

fn text_styles() -> Vec<Mark> {
    let mut styles = Vec::new();
    for color in attribute_states("#123abc") {
        for font_family in attribute_states("树 🌲 e\u{301} Serif") {
            for font_size in attribute_states("12.50px") {
                styles.push(Mark::TextStyle(TextStyleMark::from_attributes(
                    TextStyleAttributes::default()
                        .with_color(color.clone())
                        .with_font_family(font_family.clone())
                        .with_font_size(font_size),
                )));
            }
        }
    }
    styles
}

fn inline_block(
    builder: &mut NodeStoreBuilder,
    kind: NodeKind,
    marks: impl IntoIterator<Item = Mark>,
) -> NodeId {
    let runs = [
        TextRun::new("before ", MarkSet::empty()).unwrap(),
        TextRun::new("样式 e\u{301} 🌲", MarkSet::new(marks).unwrap()).unwrap(),
        TextRun::new(" after", MarkSet::empty()).unwrap(),
    ];
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new(runs).unwrap()),
        )
        .unwrap()
}

fn container(
    builder: &mut NodeStoreBuilder,
    kind: NodeKind,
    children: impl IntoIterator<Item = NodeId>,
) -> NodeId {
    builder
        .insert(kind, NodeAttrs::empty(), NodeContent::children(children))
        .unwrap()
}

fn document(mut builder: NodeStoreBuilder, block: NodeId) -> XiaomuDocument {
    let root = container(&mut builder, NodeKind::Document, [block]);
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

fn assert_refused(document: XiaomuDocument, description: &str) {
    let nodes_before: Vec<_> = document.store().iter().cloned().collect();
    let revision_before = document.revision();
    assert_eq!(
        to_markdown(&document),
        Err(MarkdownCodecError::UnsupportedMark {
            mark: description.to_owned(),
        }),
        "{nodes_before:?}"
    );
    assert_eq!(
        document.store().iter().cloned().collect::<Vec<_>>(),
        nodes_before
    );
    assert_eq!(document.revision(), revision_before);
    document.validate().unwrap();
}

#[test]
fn paragraph_refuses_all_missing_null_empty_and_value_combinations() {
    for style in text_styles() {
        let mut builder = NodeStoreBuilder::new();
        let block = inline_block(&mut builder, NodeKind::Paragraph, [style]);
        assert_refused(document(builder, block), "TextStyle");
    }
}

#[test]
fn inline_code_refuses_text_style_instead_of_dropping_it() {
    for style in text_styles() {
        let mut builder = NodeStoreBuilder::new();
        let block = inline_block(&mut builder, NodeKind::Paragraph, [Mark::Code, style]);
        assert_refused(document(builder, block), "Code + TextStyle");
    }
}

#[test]
fn supported_formatting_and_classic_link_cannot_hide_text_style() {
    for style in text_styles() {
        let mut builder = NodeStoreBuilder::new();
        let block = inline_block(
            &mut builder,
            NodeKind::Paragraph,
            [
                Mark::Bold,
                Mark::Italic,
                Mark::Strike,
                Mark::Link(LinkMark::new("https://example.invalid", None)),
                style,
            ],
        );
        assert_refused(document(builder, block), "TextStyle");
    }
}

#[test]
fn heading_plain_text_path_refuses_text_style_with_or_without_code() {
    for style in text_styles() {
        for other_marks in [vec![], vec![Mark::Code]] {
            let mut builder = NodeStoreBuilder::new();
            let block = inline_block(
                &mut builder,
                NodeKind::Heading(HeadingLevel::new(2).unwrap()),
                other_marks.into_iter().chain([style.clone()]),
            );
            assert_refused(document(builder, block), "Heading + TextStyle");
        }
    }
}

#[test]
fn code_block_refusal_names_text_style_explicitly() {
    for style in text_styles() {
        let mut builder = NodeStoreBuilder::new();
        let block = inline_block(&mut builder, NodeKind::CodeBlock, [style]);
        assert_refused(document(builder, block), "TextStyle");
    }
}

#[test]
fn nested_quotes_cannot_drop_text_style() {
    for style in text_styles() {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = inline_block(&mut builder, NodeKind::Paragraph, [style]);
        let inner = container(&mut builder, NodeKind::Quote, [paragraph]);
        let outer = container(&mut builder, NodeKind::Quote, [inner]);
        assert_refused(document(builder, outer), "TextStyle");
    }
}

#[test]
fn leading_list_item_paragraph_cannot_drop_text_style() {
    for style in text_styles() {
        for kind in [NodeKind::BulletList, NodeKind::OrderedList] {
            let mut builder = NodeStoreBuilder::new();
            let paragraph = inline_block(&mut builder, NodeKind::Paragraph, [style.clone()]);
            let item = container(&mut builder, NodeKind::ListItem, [paragraph]);
            let list = container(&mut builder, kind, [item]);
            assert_refused(document(builder, list), "TextStyle");
        }
    }
}

#[test]
fn quoted_nested_lists_cannot_drop_text_style() {
    for style in text_styles() {
        for kind in [NodeKind::BulletList, NodeKind::OrderedList] {
            let mut builder = NodeStoreBuilder::new();
            let paragraph = inline_block(&mut builder, NodeKind::Paragraph, [style.clone()]);
            let inner_item = container(&mut builder, NodeKind::ListItem, [paragraph]);
            let inner_list = container(&mut builder, kind.clone(), [inner_item]);
            let leading = inline_block(&mut builder, NodeKind::Paragraph, []);
            let outer_item = container(&mut builder, NodeKind::ListItem, [leading, inner_list]);
            let outer_list = container(&mut builder, kind, [outer_item]);
            let quote = container(&mut builder, NodeKind::Quote, [outer_list]);
            assert_refused(document(builder, quote), "TextStyle");
        }
    }
}
