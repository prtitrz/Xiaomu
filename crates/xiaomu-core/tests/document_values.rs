use xiaomu_core::{
    Error,
    document::{
        DocumentRevision, DocumentVersion, HeadingLevel, LinkMark, Mark, MarkKind, MarkSet,
        NodeKind, TextRun,
    },
};

#[test]
fn document_version_and_revision_have_distinct_semantics() {
    assert_eq!(DocumentVersion::CURRENT.as_u32(), 1);
    assert_eq!(DocumentVersion::new(7).as_u32(), 7);

    let revision = DocumentRevision::INITIAL;
    assert_eq!(revision.as_u64(), 0);
    assert_eq!(revision.next().unwrap().as_u64(), 1);
}

#[test]
fn heading_level_accepts_only_html_compatible_levels() {
    assert_eq!(HeadingLevel::new(1).unwrap().as_u8(), 1);
    assert_eq!(HeadingLevel::new(6).unwrap().as_u8(), 6);
    assert_eq!(
        HeadingLevel::new(0),
        Err(Error::InvalidHeadingLevel { level: 0 })
    );
    assert_eq!(
        HeadingLevel::new(7),
        Err(Error::InvalidHeadingLevel { level: 7 })
    );
}

#[test]
fn custom_node_kind_requires_a_stable_non_empty_key() {
    assert_eq!(
        NodeKind::custom("   "),
        Err(Error::InvalidCustomNodeKind)
    );

    let kind = NodeKind::custom("example.callout").unwrap();
    assert_eq!(kind, NodeKind::Custom("example.callout".to_owned()));
}

#[test]
fn mark_set_has_deterministic_canonical_order() {
    let marks = MarkSet::new([
        Mark::Strike,
        Mark::Bold,
        Mark::Underline,
        Mark::Italic,
        Mark::Code,
    ])
    .unwrap();

    let kinds: Vec<_> = marks.as_slice().iter().map(Mark::kind).collect();
    assert_eq!(
        kinds,
        vec![
            MarkKind::Bold,
            MarkKind::Italic,
            MarkKind::Code,
            MarkKind::Underline,
            MarkKind::Strike,
        ]
    );
}

#[test]
fn identical_duplicate_marks_are_normalized_away() {
    let marks = MarkSet::new([Mark::Bold, Mark::Bold, Mark::Italic]).unwrap();

    assert_eq!(marks.len(), 2);
    assert!(marks.contains(MarkKind::Bold));
    assert!(marks.contains(MarkKind::Italic));
}

#[test]
fn conflicting_marks_of_the_same_kind_are_rejected() {
    let first = Mark::Link(LinkMark::new("https://example.com/a", None));
    let second = Mark::Link(LinkMark::new("https://example.com/b", None));

    assert_eq!(
        MarkSet::new([first, second]),
        Err(Error::InvalidMarkSet)
    );
}

#[test]
fn link_mark_preserves_href_and_optional_title() {
    let link = LinkMark::new(
        "xiaomu://document/node",
        Some("Open referenced node".to_owned()),
    );

    assert_eq!(link.href(), "xiaomu://document/node");
    assert_eq!(link.title(), Some("Open referenced node"));
}

#[test]
fn text_run_rejects_empty_persistent_text() {
    assert_eq!(
        TextRun::new("", MarkSet::empty()),
        Err(Error::EmptyTextRun)
    );
}

#[test]
fn text_run_keeps_text_and_normalized_marks_together() {
    let marks = MarkSet::new([Mark::Italic, Mark::Bold]).unwrap();
    let run = TextRun::new("晓木🙂", marks).unwrap();

    assert_eq!(run.text().as_str(), "晓木🙂");
    assert_eq!(run.len_bytes(), "晓木🙂".len());
    assert_eq!(
        run.marks().as_slice().iter().map(Mark::kind).collect::<Vec<_>>(),
        vec![MarkKind::Bold, MarkKind::Italic]
    );
}
