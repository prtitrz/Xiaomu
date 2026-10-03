//! Markdown must preserve classic link attributes or explicitly refuse export.

use xiaomu_codec_markdown::{MarkdownCodecError, from_markdown, to_markdown};
use xiaomu_core::document::{
    HeadingLevel, InlineContent, LinkAttributes, LinkMark, Mark, MarkSet, NodeAttrs, NodeContent,
    NodeId, NodeKind, NodeStoreBuilder, StringAttribute, TextRun, XiaomuDocument,
};

const HREF: &str = "https://example.invalid/路径(树)";

fn document_with_link(link: LinkMark, code: bool, kind: NodeKind) -> (XiaomuDocument, NodeId) {
    let mut marks = vec![Mark::Link(link)];
    if code {
        marks.push(Mark::Code);
    }
    let run = TextRun::new("链接 e\u{301} 🐾", MarkSet::new(marks).unwrap()).unwrap();
    let mut builder = NodeStoreBuilder::new();
    let block = builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new([run]).unwrap()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), block)
}

fn assert_round_trip(link: LinkMark, code: bool) {
    let (document, block) = document_with_link(link, code, NodeKind::Paragraph);
    let before = document.node(block).unwrap().clone();
    let markdown = to_markdown(&document).unwrap();
    let parsed = from_markdown(&markdown).unwrap();
    let parsed_block = parsed
        .node(parsed.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    assert_eq!(
        parsed.node(parsed_block).unwrap().content(),
        before.content(),
        "{markdown}"
    );
    assert_eq!(to_markdown(&parsed).unwrap(), markdown);
    assert_eq!(document.node(block), Some(&before));
}

fn assert_refused(link: LinkMark, code: bool) {
    let (document, block) = document_with_link(link, code, NodeKind::Paragraph);
    let before = document.node(block).unwrap().clone();
    assert_eq!(
        to_markdown(&document),
        Err(MarkdownCodecError::UnsupportedMark {
            mark: "Link attributes".to_owned(),
        }),
        "code={code}, {before:?}"
    );
    assert_eq!(document.node(block), Some(&before));
}

fn classic_attributes() -> LinkAttributes {
    LinkAttributes::default().with_href(StringAttribute::Value(HREF.to_owned()))
}

fn with_attribute(key: &str, value: StringAttribute) -> LinkAttributes {
    let attributes = classic_attributes();
    match key {
        "href" => attributes.with_href(value),
        "target" => attributes.with_target(value),
        "rel" => attributes.with_rel(value),
        "class" => attributes.with_class(value),
        "title" => attributes.with_title(value),
        _ => panic!("unknown test attribute"),
    }
}

#[test]
fn missing_optional_attributes_round_trip_but_missing_href_fails_closed() {
    for code in [false, true] {
        // Every optional field is Missing in a classic link.
        assert_round_trip(LinkMark::from_attributes(classic_attributes()), code);
        for key in ["target", "rel", "class", "title"] {
            assert_round_trip(
                LinkMark::from_attributes(with_attribute(key, StringAttribute::Missing)),
                code,
            );
        }
        assert_refused(LinkMark::from_attributes(LinkAttributes::default()), code);
        assert_refused(
            LinkMark::from_attributes(
                with_attribute("href", StringAttribute::Missing)
                    .with_title(StringAttribute::Value("title".to_owned())),
            ),
            code,
        );
    }
}

#[test]
fn explicit_null_in_any_link_attribute_fails_closed() {
    for code in [false, true] {
        for key in ["href", "target", "rel", "class", "title"] {
            assert_refused(
                LinkMark::from_attributes(with_attribute(key, StringAttribute::Null)),
                code,
            );
        }
    }
}

#[test]
fn explicit_empty_and_unicode_extension_values_fail_closed() {
    for code in [false, true] {
        for key in ["target", "rel", "class"] {
            for value in ["", "树 🌲 e\u{301} עברית"] {
                assert_refused(
                    LinkMark::from_attributes(with_attribute(
                        key,
                        StringAttribute::Value(value.to_owned()),
                    )),
                    code,
                );
            }
        }
    }
}

#[test]
fn empty_and_unicode_href_and_title_values_round_trip_exactly() {
    for code in [false, true] {
        for href in ["", HREF, "https://例.invalid/树 🌲/e\u{301}"] {
            for title in [
                StringAttribute::Missing,
                StringAttribute::Value(String::new()),
                StringAttribute::Value("树 \"🌲\" \\ e\u{301} עברית".to_owned()),
            ] {
                let attributes = LinkAttributes::default()
                    .with_href(StringAttribute::Value(href.to_owned()))
                    .with_title(title);
                assert_round_trip(LinkMark::from_attributes(attributes), code);
            }
        }
    }
}

#[test]
fn classic_constructor_preserves_existing_round_trip_behavior() {
    for code in [false, true] {
        for title in [None, Some(String::new()), Some("old title".to_owned())] {
            assert_round_trip(LinkMark::new(HREF, title), code);
        }
    }
}

#[test]
fn heading_plain_text_export_cannot_silently_drop_link_attributes() {
    let links = [
        LinkMark::new(HREF, None),
        LinkMark::from_attributes(
            classic_attributes().with_target(StringAttribute::Value("_blank".to_owned())),
        ),
        LinkMark::from_attributes(classic_attributes().with_title(StringAttribute::Null)),
    ];
    for code in [false, true] {
        for link in &links {
            let (document, heading) = document_with_link(
                link.clone(),
                code,
                NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            );
            let before = document.node(heading).unwrap().clone();
            assert_eq!(
                to_markdown(&document),
                Err(MarkdownCodecError::UnsupportedMark {
                    mark: "Heading + Link".to_owned(),
                })
            );
            assert_eq!(document.node(heading), Some(&before));
        }
    }
}
