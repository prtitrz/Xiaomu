mod support;
use support::{assert_semantics, document, run};
use xiaomu_codec_markdown::{MarkdownCodecError, from_markdown, to_markdown};
use xiaomu_core::document::{HeadingLevel, LinkMark, Mark, NodeKind, XiaomuDocument};

fn round_trip(doc: &XiaomuDocument) {
    let markdown = to_markdown(doc).unwrap();
    let imported = from_markdown(&markdown).unwrap();
    assert_semantics(doc, &imported);
    assert_eq!(to_markdown(&imported).unwrap(), markdown);
}

#[test]
fn heading_preserves_supported_marks_and_refuses_underline() {
    for level in 1..=6 {
        for marks in [
            vec![Mark::Bold],
            vec![Mark::Italic],
            vec![Mark::Strike],
            vec![Mark::Code],
            vec![Mark::Bold, Mark::Italic, Mark::Strike],
        ] {
            let doc = document(
                NodeKind::Heading(HeadingLevel::new(level).unwrap()),
                vec![run("格式 e\u{301} 🌲", marks)],
                &[],
            );
            round_trip(&doc);
        }
        let doc = document(
            NodeKind::Heading(HeadingLevel::new(level).unwrap()),
            vec![run("heading", [Mark::Underline])],
            &[],
        );
        assert!(matches!(
            to_markdown(&doc),
            Err(MarkdownCodecError::UnsupportedMark { .. })
        ));
    }
    round_trip(&from_markdown("## **bold**\n").unwrap());
}

#[test]
fn code_padding_preserves_spaces_and_edge_backticks() {
    for text in [
        " a ",
        "  a  ",
        " a",
        "a ",
        " ",
        "   ",
        "`a`",
        "`",
        "`a",
        "a`",
        "` `",
        "树 ` 🐾",
    ] {
        for prefix in ["", "before "] {
            for link in [false, true] {
                let mut marks = vec![Mark::Code];
                if link {
                    marks.push(Mark::Link(LinkMark::new("https://example.invalid", None)));
                }
                let mut runs = vec![];
                if !prefix.is_empty() {
                    runs.push(run(prefix, []));
                }
                runs.push(run(text, marks));
                round_trip(&document(NodeKind::Paragraph, runs, &[]));
            }
        }
    }
}

#[test]
fn long_inline_fences_never_become_fenced_blocks() {
    for text in ["``", "a``b", "`a``b```", "``a``"] {
        let doc = document(NodeKind::Paragraph, vec![run(text, [Mark::Code])], &[]);
        assert!(matches!(
            to_markdown(&doc),
            Err(MarkdownCodecError::UnsupportedMark { .. })
        ));
        round_trip(&document(
            NodeKind::Paragraph,
            vec![run("prefix ", []), run(text, [Mark::Code])],
            &[],
        ));
        round_trip(&document(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            vec![run(text, [Mark::Code])],
            &[],
        ));
    }
}

#[test]
fn trailing_lf_refuses_without_mutating_in_all_containers() {
    for containers in [
        vec![],
        vec![NodeKind::Quote],
        vec![NodeKind::ListItem, NodeKind::BulletList],
        vec![NodeKind::ListItem, NodeKind::OrderedList, NodeKind::Quote],
    ] {
        for text in ["a\n", "a\n\n", "a\nb\n"] {
            for marks in [vec![], vec![Mark::Bold], vec![Mark::Italic]] {
                let doc = document(NodeKind::Paragraph, vec![run(text, marks)], &containers);
                let before = doc.clone();
                assert!(matches!(
                    to_markdown(&doc),
                    Err(MarkdownCodecError::HardBreakNotRepresentable { .. })
                ));
                assert_semantics(&doc, &before);
            }
        }
        for marks in [vec![], vec![Mark::Bold], vec![Mark::Italic]] {
            round_trip(&document(
                NodeKind::Paragraph,
                vec![run("a\nb", marks)],
                &containers,
            ));
        }
    }
}

#[test]
fn adjacent_mark_combinations_preserve_exact_runs_or_refuse() {
    let marks = [
        vec![],
        vec![Mark::Bold],
        vec![Mark::Italic],
        vec![Mark::Strike],
        vec![Mark::Code],
        vec![Mark::Bold, Mark::Italic],
        vec![Mark::Link(LinkMark::new("https://x.invalid", None))],
    ];
    for a in &marks {
        for b in &marks {
            for text in ["a", " a ", "`a`", "a\nb", "树 * 🐾"] {
                let doc = document(
                    NodeKind::Paragraph,
                    vec![run(text, a.clone()), run("tail", b.clone())],
                    &[],
                );
                if let Ok(markdown) = to_markdown(&doc) {
                    let parsed = from_markdown(&markdown).expect(&markdown);
                    assert_semantics(&doc, &parsed);
                }
            }
        }
    }
}
