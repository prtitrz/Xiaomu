//! Plain text and transient preedit projection. Runtime owns mark inheritance.

use xiaomu_core::document::{InlineContent, Mark, MarkKind, MarkSet, TextStyleAttributes};

/// One styled span of displayed text; offsets never enter canonical storage.
#[derive(Clone, Debug)]
pub(crate) struct DisplaySegment {
    pub(crate) start: usize,
    pub(crate) text: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) code: bool,
    pub(crate) link: bool,
    pub(crate) text_style: Option<TextStyleAttributes>,
}

impl DisplaySegment {
    pub(super) fn from_marks(start: usize, text: &str, marks: &MarkSet) -> Self {
        Self {
            start,
            text: text.to_owned(),
            bold: marks.contains(MarkKind::Bold),
            italic: marks.contains(MarkKind::Italic),
            underline: marks.contains(MarkKind::Underline),
            strike: marks.contains(MarkKind::Strike),
            code: marks.contains(MarkKind::Code),
            link: marks.contains(MarkKind::Link),
            text_style: marks.as_slice().iter().find_map(|mark| match mark {
                Mark::TextStyle(style) => Some(style.attributes().clone()),
                _ => None,
            }),
        }
    }

    /// Repair the old default-only overlay: all effective marks now match
    /// committed text, with an additional transient IME underline. Unmarked
    /// input retains its previous default appearance. This is view-only.
    pub(super) fn preedit(start: usize, text: &str, marks: &MarkSet) -> Self {
        let mut segment = Self::from_marks(start, text, marks);
        segment.underline = true;
        segment
    }
}

pub(super) fn project_display_content(
    inline: &InlineContent,
    composition: Option<(std::ops::Range<usize>, &str, &MarkSet)>,
) -> (String, Vec<DisplaySegment>) {
    let (base_start, base_end, preedit) = composition
        .as_ref()
        .map(|(range, text, _)| (range.start, range.end, *text))
        .unwrap_or((usize::MAX, usize::MAX, ""));
    let replaced_len = base_end.saturating_sub(base_start);
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;
        let mut push_piece = |start: usize, end: usize, display_start: usize| {
            if start < end {
                segments.push(DisplaySegment::from_marks(
                    display_start,
                    &run.text().as_str()[start - run_start..end - run_start],
                    run.marks(),
                ));
            }
        };
        push_piece(run_start, run_end.min(base_start), run_start);
        let suffix_start = run_start.max(base_end);
        let suffix_display_start = suffix_start.saturating_sub(replaced_len) + preedit.len();
        push_piece(suffix_start, run_end, suffix_display_start);
    }
    if let Some((range, text, marks)) = composition {
        segments.push(DisplaySegment::preedit(range.start, text, marks));
    }
    segments.sort_by_key(|segment| segment.start);
    normalize_segments(segments)
}

pub(super) fn normalize_segments(
    mut segments: Vec<DisplaySegment>,
) -> (String, Vec<DisplaySegment>) {
    let mut text = String::new();
    for segment in &mut segments {
        segment.start = text.len();
        text.push_str(&segment.text);
    }
    (text, segments)
}
