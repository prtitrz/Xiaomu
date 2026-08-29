use xiaomu_core::document::{InlineContent, Mark, MarkSet, TextRun};

use super::project_display_content;

fn inline(runs: &[(&str, MarkSet)]) -> InlineContent {
    InlineContent::new(
        runs.iter()
            .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap()),
    )
    .unwrap()
}

#[test]
fn collapsed_composition_is_spliced_at_the_caret() {
    let content = inline(&[("before-after", MarkSet::empty())]);

    let (text, segments) = project_display_content(&content, Some((7..7, "nihao")));

    assert_eq!(text, "before-nihaoafter");
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].text, "before-");
    assert_eq!(segments[1].text, "nihao");
    assert!(segments[1].underline);
    assert_eq!(segments[2].text, "after");
}

#[test]
fn composition_replaces_a_range_across_styled_runs() {
    let bold = MarkSet::new([Mark::Bold]).unwrap();
    let content = inline(&[
        ("ab", MarkSet::empty()),
        ("中文", bold),
        ("cd", MarkSet::empty()),
    ]);

    let (text, segments) = project_display_content(&content, Some((1..8, "X")));

    assert_eq!(text, "aXcd");
    assert_eq!(
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>(),
        ["a", "X", "cd"]
    );
    assert!(!segments[0].bold);
    assert!(segments[1].underline);
    assert!(!segments[2].bold);
}

#[test]
fn idle_projection_preserves_text_and_run_styles() {
    let underline = MarkSet::new([Mark::Underline]).unwrap();
    let content = inline(&[("plain", MarkSet::empty()), ("marked", underline)]);

    let (text, segments) = project_display_content(&content, None);

    assert_eq!(text, "plainmarked");
    assert_eq!(segments.len(), 2);
    assert!(!segments[0].underline);
    assert!(segments[1].underline);
}

#[test]
fn multiline_projection_preserves_canonical_lf_and_segment_offsets() {
    let bold = MarkSet::new([Mark::Bold]).unwrap();
    let content = inline(&[("alpha\n", MarkSet::empty()), ("beta", bold)]);

    let (text, segments) = project_display_content(&content, None);

    assert_eq!(text, "alpha\nbeta");
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].start, 0);
    assert_eq!(segments[0].text, "alpha\n");
    assert_eq!(segments[1].start, 6);
    assert_eq!(segments[1].text, "beta");
    assert!(segments[1].bold);
}

#[test]
fn composition_after_hard_break_keeps_lf_in_virtual_projection() {
    let content = inline(&[("a\nb", MarkSet::empty())]);

    let (text, segments) = project_display_content(&content, Some((2..2, "中")));

    assert_eq!(text, "a\n中b");
    assert_eq!(
        segments
            .iter()
            .map(|segment| (segment.start, segment.text.as_str()))
            .collect::<Vec<_>>(),
        [(0, "a\n"), (2, "中"), (5, "b")]
    );
    assert!(segments[1].underline);
}
