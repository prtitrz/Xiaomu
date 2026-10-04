//! Unsupported typed/marked atoms must never disappear during Markdown export.

use xiaomu_codec_markdown::{MarkdownCodecError, from_markdown, to_markdown};
use xiaomu_core::document::{
    AtomKind, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent, LinkAttributes,
    LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    TextStyleAttributes, TextStyleMark, XiaomuDocument,
};
use xiaomu_core::text::TextOffset;

fn with_atom(parent: NodeKind, kind: AtomKind, content: InlineAtomContent) -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(kind),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(content),
        )
        .unwrap();
    let leaf = builder
        .insert(
            parent,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("a\nb", MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([leaf]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

#[test]
fn all_markdown_inline_paths_explicitly_reject_typed_and_marked_atoms() {
    let marks = MarkSet::new([
        Mark::Code,
        Mark::Link(LinkMark::from_attributes(LinkAttributes::default())),
        Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default(),
        )),
    ])
    .unwrap();
    for parent in [
        NodeKind::Paragraph,
        NodeKind::Heading(HeadingLevel::new(2).unwrap()),
        NodeKind::CodeBlock,
    ] {
        for (kind, content) in [
            (AtomKind::hard_break(), InlineAtomContent::hard_break()),
            (
                AtomKind::hard_break(),
                InlineAtomContent::hard_break().with_marks(marks.clone()),
            ),
            (
                AtomKind::new("hardBreak").unwrap(),
                InlineAtomContent::new("\n")
                    .unwrap()
                    .with_marks(marks.clone()),
            ),
            (
                AtomKind::new("mention").unwrap(),
                InlineAtomContent::new("@Ann").unwrap(),
            ),
        ] {
            let description = format!("InlineAtom({})", kind.as_str());
            let document = with_atom(parent.clone(), kind, content);
            let original = document.clone();
            assert_eq!(
                to_markdown(&document),
                Err(MarkdownCodecError::UnsupportedNodeKind { kind: description })
            );
            assert_eq!(document.store(), original.store());
            assert_eq!(document.root(), original.root());
            assert_eq!(document.revision(), original.revision());
        }
    }
}

#[test]
fn plain_code_newlines_stay_unchanged_and_code_run_marks_remain_rejected() {
    let markdown = "```\na\nb\n```\n";
    assert_eq!(
        to_markdown(&from_markdown(markdown).unwrap()).unwrap(),
        markdown
    );
    for mark in [Mark::Bold, Mark::Code] {
        let mut builder = NodeStoreBuilder::new();
        let leaf = builder
            .insert(
                NodeKind::CodeBlock,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([
                        TextRun::new("code", MarkSet::new([mark]).unwrap()).unwrap()
                    ])
                    .unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([leaf]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        assert!(matches!(
            to_markdown(&document),
            Err(MarkdownCodecError::UnsupportedMark { .. })
        ));
    }
}
