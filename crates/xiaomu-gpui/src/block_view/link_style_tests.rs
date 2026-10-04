//! Link decoration is a view projection, never a URL activation or text rewrite.
use super::text_runs;
use crate::block_view::project_display_content;
use xiaomu_core::document::{
    InlineContent, LinkAttributes, LinkMark, Mark, MarkSet, StringAttribute, TextRun,
};

fn inline() -> InlineContent {
    InlineContent::new([
        TextRun::new(
            "链接🙂",
            MarkSet::new([
                Mark::Link(LinkMark::from_attributes(
                    LinkAttributes::default()
                        .with_href(StringAttribute::Null)
                        .with_title(StringAttribute::Value("保留".into())),
                )),
                Mark::Bold,
            ])
            .unwrap(),
        )
        .unwrap(),
        TextRun::new(" plain", MarkSet::empty()).unwrap(),
    ])
    .unwrap()
}

#[test]
fn link_decoration_preserves_canonical_text_attrs_and_other_marks() {
    let content = inline();
    let before = content.clone();
    let (text, segments) = project_display_content(&content, None);
    assert_eq!(text, "链接🙂 plain");
    assert!(segments[0].link && segments[0].bold);
    assert!(!segments[1].link);
    let color = gpui::rgba(0x111111ff).into();
    let fonts = crate::block_view::text_style::FontCatalog::from_names(&[]);
    let runs = text_runs(&segments, gpui::font("sans-serif"), color, &fonts);
    assert!(runs[0].underline.is_some());
    assert_ne!(runs[0].color, color);
    assert_eq!(runs[1].color, color);
    assert!(runs[1].underline.is_none());
    assert_eq!(content, before);
}

#[test]
fn explicit_empty_pending_marks_keep_preedit_plain_but_link_neighbors_styled() {
    let content = inline();
    let (text, segments) =
        project_display_content(&content, Some((3..3, "预编辑", &MarkSet::empty())));
    assert_eq!(text, "链预编辑接🙂 plain");
    let preedit = segments
        .iter()
        .find(|segment| segment.text == "预编辑")
        .unwrap();
    assert!(preedit.underline && !preedit.link);
    assert!(segments.first().unwrap().link);
    assert!(
        segments
            .iter()
            .any(|segment| segment.text == "接🙂" && segment.link)
    );
    assert_eq!(project_display_content(&content, None).0, "链接🙂 plain");
}
