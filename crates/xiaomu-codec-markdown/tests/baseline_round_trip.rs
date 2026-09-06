//! P4.9 baseline Markdown round-trip gate.
//!
//! The codec covers the built-in semantics that exist at the P4 closeout and
//! refuses everything else instead of silently dropping it. These tests pin
//! the canonical export form, the import strictness, and the fail-closed
//! preservation policy for unknown attrs and host asset images.

use std::collections::BTreeMap;

use xiaomu_codec_markdown::{MarkdownCodecError, from_markdown, to_markdown};
use xiaomu_core::document::{
    HeadingLevel, ImageAttrs, ImageSource, InlineContent, LinkMark, Mark, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};

fn run(text: &str, marks: MarkSet) -> TextRun {
    TextRun::new(text, marks).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, runs: Vec<TextRun>) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new(runs).unwrap()),
        )
        .unwrap()
}

fn heading(builder: &mut NodeStoreBuilder, level: u8, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Heading(HeadingLevel::new(level).unwrap()),
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new([run(text, MarkSet::empty())]).unwrap()),
        )
        .unwrap()
}

fn hr(builder: &mut NodeStoreBuilder) -> NodeId {
    builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap()
}

fn image(builder: &mut NodeStoreBuilder, source: ImageSource, title: Option<String>) -> NodeId {
    let attrs = ImageAttrs::new(source, "alt text".to_owned(), title, None, None)
        .unwrap()
        .to_attrs()
        .unwrap();
    builder
        .insert(NodeKind::Image, attrs, NodeContent::Atomic)
        .unwrap()
}

fn code_block(builder: &mut NodeStoreBuilder, language: Option<&str>, body: &str) -> NodeId {
    let mut values = BTreeMap::new();
    if let Some(language) = language {
        values.insert(
            "language".to_owned(),
            xiaomu_core::document::AttrValue::String(language.to_owned()),
        );
    }
    builder
        .insert(
            NodeKind::CodeBlock,
            NodeAttrs::new(values).unwrap(),
            NodeContent::Inline(InlineContent::new([run(body, MarkSet::empty())]).unwrap()),
        )
        .unwrap()
}

fn document(builder: NodeStoreBuilder, children: Vec<NodeId>) -> XiaomuDocument {
    let mut builder = builder;
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

fn marks(marks: Vec<Mark>) -> MarkSet {
    MarkSet::new(marks).unwrap()
}

/// Walks one document into a comparable structural description.
fn describe(document: &XiaomuDocument) -> Vec<String> {
    fn walk(document: &XiaomuDocument, id: NodeId, depth: usize, out: &mut Vec<String>) {
        let node = document.node(id).unwrap();
        let indent = "  ".repeat(depth);
        let detail = match node.content() {
            NodeContent::Inline(inline) => {
                if matches!(node.kind(), NodeKind::CodeBlock) {
                    inline
                        .runs()
                        .iter()
                        .map(|run| run.text().as_str())
                        .collect::<String>()
                } else {
                    inline
                        .runs()
                        .iter()
                        .map(|run| {
                            let marks: Vec<String> =
                                run.marks().as_slice().iter().map(mark_name).collect();
                            format!("{}<{}>", run.text().as_str(), marks.join("+"))
                        })
                        .collect::<Vec<_>>()
                        .join("\u{1}")
                }
            }
            NodeContent::Children(children) => {
                for &child in children {
                    walk(document, child, depth + 1, out);
                }
                return;
            }
            NodeContent::Atomic => match node.kind() {
                NodeKind::Image => {
                    let image = ImageAttrs::from_attrs(node.attrs()).unwrap();
                    format!(
                        "image url={} alt={} title={:?}",
                        image.source().value(),
                        image.alt(),
                        image.title()
                    )
                }
                _ => "atomic".to_owned(),
            },
            NodeContent::InlineAtom(content) => content.fallback_text().to_owned(),
            _ => String::new(),
        };
        out.push(format!("{indent}{:?} {}", node.kind(), detail));
    }

    let mut out = Vec::new();
    walk(document, document.root(), 0, &mut out);
    out
}

fn mark_name(mark: &Mark) -> String {
    match mark {
        Mark::Bold => "Bold".to_owned(),
        Mark::Italic => "Italic".to_owned(),
        Mark::Code => "Code".to_owned(),
        Mark::Underline => "Underline".to_owned(),
        Mark::Strike => "Strike".to_owned(),
        Mark::Link(_) => "Link".to_owned(),
        _ => "Other".to_owned(),
    }
}

#[test]
fn rich_document_round_trips_through_canonical_markdown() {
    let mut builder = NodeStoreBuilder::new();
    let bold = run("bold", marks(vec![Mark::Bold]));
    let code = run("code()", marks(vec![Mark::Code]));
    let plain = run(" tail ", MarkSet::empty());
    let link = run(
        "linked",
        marks(vec![Mark::Link(LinkMark::new(
            "https://example.com/a(b)",
            Some("t \"x\"".to_owned()),
        ))]),
    );
    let strike = run("struck", marks(vec![Mark::Strike]));
    let mixed = run("all", marks(vec![Mark::Bold, Mark::Italic, Mark::Strike]));

    let first = paragraph(&mut builder, vec![bold, code, plain, link, strike, mixed]);
    let hard_break = paragraph(&mut builder, vec![run("a\nb", MarkSet::empty())]);
    let heading = heading(&mut builder, 3, "标题 heading");
    let rule = hr(&mut builder);
    let image = image(
        &mut builder,
        ImageSource::ExternalUrl("https://example.com/cover.png".to_owned()),
        Some("cover".to_owned()),
    );
    let code_block = code_block(&mut builder, Some("rust"), "fn main() {\n    // ```\n}");

    let item_a_text = paragraph(&mut builder, vec![run("item a", MarkSet::empty())]);
    let item_a = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([item_a_text]),
        )
        .unwrap();
    let item_b = paragraph(&mut builder, vec![run("item *b*", MarkSet::empty())]);
    let nested_text = paragraph(&mut builder, vec![run("nested", MarkSet::empty())]);
    let nested_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([nested_text]),
        )
        .unwrap();
    let nested_list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([nested_item]),
        )
        .unwrap();
    let item_c = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([item_b, nested_list]),
        )
        .unwrap();
    let bullet_list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_a, item_c]),
        )
        .unwrap();

    let quote_paragraph = paragraph(&mut builder, vec![run("quoted", MarkSet::empty())]);
    let quote_rule = hr(&mut builder);
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([quote_paragraph, quote_rule]),
        )
        .unwrap();

    let doc = document(
        builder,
        vec![
            first,
            hard_break,
            heading,
            rule,
            image,
            code_block,
            bullet_list,
            quote,
        ],
    );

    let markdown = to_markdown(&doc).unwrap();
    let expected = "\
**bold**``code()`` tail [linked](https://example.com/a\\(b\\) \"t \\\"x\\\"\")~~struck~~**~~*all*~~**

a\\
b

### 标题 heading

---

![alt text](https://example.com/cover.png \"cover\")

````rust
fn main() {
    // ```
}
````

- item a
- item \\*b\\*
  - nested

> quoted
>
> ---
";
    assert_eq!(markdown, expected);

    let reparsed = from_markdown(&markdown).unwrap();
    let again = to_markdown(&reparsed).unwrap();
    assert_eq!(again, markdown, "parse ∘ export must be a fixed point");

    let described = describe(&reparsed).join("\n");
    assert!(described.contains("bold<Bold>"));
    assert!(described.contains("code()<Code>"));
    assert!(described.contains("linked<Link>"));
    assert!(described.contains("struck<Strike>"));
    assert!(described.contains("a\nb"), "hard break preserved");
    assert!(described.contains("标题 heading"));
    assert!(described.contains("HorizontalRule"));
    assert!(described.contains("image url=https://example.com/cover.png"));
    assert!(described.contains("fn main()"));
    assert!(described.contains("quoted"));
}

#[test]
fn unicode_text_and_alt_survive_the_round_trip() {
    let mut builder = NodeStoreBuilder::new();
    let text = run("中文 🎉 remobber مرحبا", MarkSet::empty());
    let paragraph = paragraph(&mut builder, vec![text]);
    let image = builder
        .insert(
            NodeKind::Image,
            ImageAttrs::new(
                ImageSource::ExternalUrl("https://example.com/图片.png".to_owned()),
                "截图 📸".to_owned(),
                None,
                None,
                None,
            )
            .unwrap()
            .to_attrs()
            .unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let doc = document(builder, vec![paragraph, image]);

    let markdown = to_markdown(&doc).unwrap();
    assert_eq!(
        markdown,
        "中文 🎉 remobber مرحبا\n\n![截图 📸](https://example.com/图片.png)\n"
    );

    let reparsed = from_markdown(&markdown).unwrap();
    assert_eq!(to_markdown(&reparsed).unwrap(), markdown);
    assert!(
        describe(&reparsed)
            .iter()
            .any(|line| line.contains("中文 🎉"))
    );
    assert!(
        describe(&reparsed)
            .iter()
            .any(|line| line.contains("截图 📸"))
    );
}

#[test]
fn escaped_punctuation_round_trips_exactly() {
    let mut builder = NodeStoreBuilder::new();
    let tricky = "a*b_c [link](x) `tick` \\ back #tag - dash + plus . dot ! bang <gt> [br] tilde~";
    let paragraph = paragraph(&mut builder, vec![run(tricky, MarkSet::empty())]);
    let doc = document(builder, vec![paragraph]);

    let markdown = to_markdown(&doc).unwrap();
    let reparsed = from_markdown(&markdown).unwrap();
    assert_eq!(to_markdown(&reparsed).unwrap(), markdown);
    assert!(describe(&reparsed)[0].contains(tricky));
}

#[test]
fn asset_ref_image_export_fails_closed() {
    let mut builder = NodeStoreBuilder::new();
    let image = image(
        &mut builder,
        ImageSource::AssetRef("host-media/2026/report-cover".to_owned()),
        None,
    );
    let doc = document(builder, vec![image]);

    assert_eq!(
        to_markdown(&doc),
        Err(MarkdownCodecError::AssetImageNotExportable)
    );
}

#[test]
fn unknown_attrs_export_fails_closed_and_preserves_them() {
    let mut builder = NodeStoreBuilder::new();
    let mut values = BTreeMap::new();
    values.insert(
        "data-x-extension-tag".to_owned(),
        xiaomu_core::document::AttrValue::String("v1".to_owned()),
    );
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::new(values).unwrap(),
            NodeContent::Inline(InlineContent::new([run("text", MarkSet::empty())]).unwrap()),
        )
        .unwrap();
    let doc = document(builder, vec![paragraph]);

    let error = to_markdown(&doc).unwrap_err();
    assert_eq!(
        error,
        MarkdownCodecError::UnsupportedAttributes {
            kind: "Paragraph".to_owned(),
            keys: vec!["data-x-extension-tag".to_owned()],
        }
    );
    // The canonical document is untouched behind the codec boundary.
    let key = doc
        .node(paragraph)
        .unwrap()
        .attrs()
        .get("data-x-extension-tag")
        .unwrap();
    assert_eq!(
        key,
        &xiaomu_core::document::AttrValue::String("v1".to_owned())
    );
}

#[test]
fn image_dimensions_and_unknown_attrs_fail_export() {
    let mut builder = NodeStoreBuilder::new();
    let attrs = ImageAttrs::new(
        ImageSource::ExternalUrl("https://example.com/a.png".to_owned()),
        "alt".to_owned(),
        None,
        Some(640),
        Some(480),
    )
    .unwrap()
    .to_attrs()
    .unwrap();
    let image = builder
        .insert(NodeKind::Image, attrs, NodeContent::Atomic)
        .unwrap();
    let doc = document(builder, vec![image]);

    assert_eq!(
        to_markdown(&doc),
        Err(MarkdownCodecError::UnsupportedAttributes {
            kind: "Image".to_owned(),
            keys: vec!["width".to_owned(), "height".to_owned()],
        })
    );
}

#[test]
fn unsupported_content_fails_export() {
    // Underline mark.
    let mut builder = NodeStoreBuilder::new();
    let underlined = paragraph(&mut builder, vec![run("u", marks(vec![Mark::Underline]))]);
    let doc = document(builder, vec![underlined]);
    assert_eq!(
        to_markdown(&doc),
        Err(MarkdownCodecError::UnsupportedMark {
            mark: "Underline".to_owned(),
        })
    );

    // Inline atom placement.
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(xiaomu_core::document::AtomKind::new("mention").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(
                xiaomu_core::document::InlineAtomContent::new("@xiaomu").unwrap(),
            ),
        )
        .unwrap();
    let placement = xiaomu_core::document::InlineAtomPlacement::new(atom, offset_zero());
    let mixed = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms([run("hi ", MarkSet::empty())], [placement]).unwrap(),
            ),
        )
        .unwrap();
    let doc = document(builder, vec![mixed]);
    assert_eq!(
        to_markdown(&doc),
        Err(MarkdownCodecError::UnsupportedNodeKind {
            kind: "InlineAtom(mention)".to_owned(),
        })
    );

    // Extension node kind.
    let mut builder = NodeStoreBuilder::new();
    let custom = builder
        .insert(
            NodeKind::Custom("x-spoiler".to_owned()),
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let doc = document(builder, vec![custom]);
    assert_eq!(
        to_markdown(&doc),
        Err(MarkdownCodecError::UnsupportedNodeKind {
            kind: "Custom(x-spoiler)".to_owned(),
        })
    );

    // Empty paragraph.
    let mut builder = NodeStoreBuilder::new();
    let empty = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let doc = document(builder, vec![empty]);
    assert_eq!(to_markdown(&doc), Err(MarkdownCodecError::EmptyParagraph));
}

fn offset_zero() -> xiaomu_core::text::TextOffset {
    xiaomu_core::text::TextBuffer::from_string("x".to_owned())
        .offset_at(0)
        .unwrap()
}

#[test]
fn parse_refuses_ambiguous_or_unrepresentable_source() {
    let cases = [
        // Setext headings.
        "text\n---\n",
        "text\n===\n",
        // Indented code.
        "text\n\n    indented\n",
        // Lazy quote continuation.
        "> quoted\nlazy\n",
        // Unclosed fence.
        "```rust\nfn main() {}\n",
        // Inline image.
        "before ![alt](url) after\n",
        // Malformed image line.
        "![alt](url\n",
        // Empty list item.
        "- \n",
        // Unclosed emphasis is literal, but empty links are rejected.
        "[](https://a)\n",
    ];
    for source in cases {
        assert!(
            from_markdown(source).is_err(),
            "expected rejection for {source:?}"
        );
    }
}

#[test]
fn parse_accepts_lenient_import_shapes() {
    // Unclosed emphasis is read literally, matching CommonMark fallbacks.
    let doc = from_markdown("a * b * c\n").unwrap();
    assert!(describe(&doc)[0].contains("a * b * c"));

    // Numbered lists renumber on export: canonical has no start attribute.
    let doc = from_markdown("3. one\n4. two\n").unwrap();
    assert_eq!(to_markdown(&doc).unwrap(), "1. one\n2. two\n");

    // Alternative bullet characters normalize to `- `.
    let doc = from_markdown("* one\n+ two\n").unwrap();
    assert_eq!(to_markdown(&doc).unwrap(), "- one\n- two\n");

    // The trailing two-space hard break form is accepted.
    let doc = from_markdown("a  \nb\n").unwrap();
    assert!(describe(&doc)[0].contains("a\nb"));

    // An empty source is an empty document.
    let doc = from_markdown("").unwrap();
    assert_eq!(to_markdown(&doc).unwrap(), "");
}

#[test]
fn code_block_without_language_round_trips() {
    let mut builder = NodeStoreBuilder::new();
    let block = code_block(&mut builder, None, "plain\n```\ninner\n");
    let doc = document(builder, vec![block]);

    let markdown = to_markdown(&doc).unwrap();
    assert_eq!(markdown, "````\nplain\n```\ninner\n````\n");
    let reparsed = from_markdown(&markdown).unwrap();
    assert_eq!(to_markdown(&reparsed).unwrap(), markdown);
}
