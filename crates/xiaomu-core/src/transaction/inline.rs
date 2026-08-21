//! Piece-based editing over inline content.
//!
//! Run segmentation is an internal detail: these helpers split runs at range
//! boundaries, edit the affected pieces, and rebuild normalized
//! `InlineContent`. User-visible coordinates stay document text offsets.

use crate::document::{InlineContent, Mark, MarkKind, MarkSet, TextRun};
use crate::text::TextRange;
use crate::{Error, Result};

/// One contiguous piece of inline text carrying one mark set.
struct Piece {
    marks: MarkSet,
    text: String,
}

impl Piece {
    fn new(marks: MarkSet, text: impl Into<String>) -> Self {
        Self {
            marks,
            text: text.into(),
        }
    }
}

/// Validates that both range endpoints are usable coordinates of `inline`.
fn validate_inline_range(inline: &InlineContent, range: TextRange) -> Result<()> {
    if range.start() > range.end() {
        return Err(Error::InvalidTextRange {
            start: range.start().as_usize(),
            end: range.end().as_usize(),
        });
    }

    inline.validate_offset(range.start())?;
    inline.validate_offset(range.end())
}

/// Splits inline content into pieces that concatenate to the full original
/// text in order, with additional cuts at both range boundaries.
fn split_pieces(inline: &InlineContent, range: TextRange) -> Result<Vec<Piece>> {
    validate_inline_range(inline, range)?;

    let mut pieces: Vec<Piece> = Vec::new();
    let mut cursor = 0usize;

    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;

        let run_text = run.text().as_str();
        let marks = run.marks().clone();
        let start = range.start().as_usize();
        let end = range.end().as_usize();

        // Untouched prefix before the affected span.
        if run_start < start {
            let cut = run_end.min(start);
            pieces.push(Piece::new(
                marks.clone(),
                run_text[..cut - run_start].to_owned(),
            ));
        }

        // Affected region of this run, if the run intersects the range.
        if run_end > start && run_start < end {
            let from = start.max(run_start) - run_start;
            let to = end.min(run_end) - run_start;
            pieces.push(Piece::new(marks.clone(), run_text[from..to].to_owned()));
        }

        // Untouched suffix after the affected span.
        if run_end > end {
            pieces.push(Piece::new(marks, run_text[end - run_start..].to_owned()));
        }
    }

    Ok(pieces)
}

fn rebuild(pieces: Vec<Piece>) -> Result<InlineContent> {
    let mut runs = Vec::new();
    for piece in pieces {
        if !piece.text.is_empty() {
            runs.push(TextRun::new(piece.text, piece.marks)?);
        }
    }
    InlineContent::new(runs)
}

/// Returns inline content with `range` replaced by `replacement`.
///
/// The replacement inherits the marks of the piece containing
/// `range.start`, keeping continuous typing behavior deterministic.
pub fn replace_text(
    inline: &InlineContent,
    range: TextRange,
    replacement: &str,
) -> Result<InlineContent> {
    validate_inline_range(inline, range)?;

    let mut output: Vec<Piece> = Vec::new();
    let mut offset = 0usize;
    let mut replaced = false;

    for run in inline.runs() {
        let start = offset;
        let end = start + run.len_bytes();
        offset = end;

        let range_start = range.start().as_usize();
        let range_end = range.end().as_usize();

        // Prefix before the affected span.
        if start < range_start {
            let cut = end.min(range_start);
            output.push(Piece::new(
                run.marks().clone(),
                &run.text().as_str()[..cut - start],
            ));
        }

        // Replacement goes where the affected span begins.
        if !replaced && range_start <= end {
            let inherited = run.marks().clone();
            output.push(Piece::new(inherited.clone(), replacement));
            replaced = true;
        }

        // Suffix after the affected span.
        if end > range_end {
            let from = range_end.max(start) - start;
            output.push(Piece::new(
                run.marks().clone(),
                &run.text().as_str()[from..],
            ));
        }
    }

    // Empty inline content or a range starting at the very end.
    if !replaced {
        let inherited = inline
            .runs()
            .last()
            .map(|run| run.marks().clone())
            .unwrap_or_else(MarkSet::empty);
        output.push(Piece::new(inherited, replacement));
    }

    rebuild(output)
}

/// Returns inline content with `mark` applied to `range`.
///
/// An existing mark of the same kind inside the range is replaced so a run
/// never carries two competing values for one semantic mark.
pub fn add_mark(inline: &InlineContent, range: TextRange, mark: Mark) -> Result<InlineContent> {
    let mut pieces = split_pieces(inline, range)?;
    let mut offset = 0usize;
    for piece in &mut pieces {
        let start = offset;
        let end = start + piece.text.len();
        offset = end;

        if start >= range.start().as_usize()
            && end <= range.end().as_usize()
            && !piece.text.is_empty()
        {
            let without_kind: Vec<Mark> = piece
                .marks
                .as_slice()
                .iter()
                .filter(|existing| existing.kind() != mark.kind())
                .cloned()
                .collect();
            piece.marks = MarkSet::new(without_kind.into_iter().chain([mark.clone()]))?;
        }
    }

    rebuild(pieces)
}

/// Returns inline content with all marks of `kind` removed from `range`.
pub fn remove_mark(
    inline: &InlineContent,
    range: TextRange,
    kind: MarkKind,
) -> Result<InlineContent> {
    let mut pieces = split_pieces(inline, range)?;
    let mut offset = 0usize;
    for piece in &mut pieces {
        let start = offset;
        let end = start + piece.text.len();
        offset = end;

        if start >= range.start().as_usize()
            && end <= range.end().as_usize()
            && !piece.text.is_empty()
        {
            let remaining: Vec<Mark> = piece
                .marks
                .as_slice()
                .iter()
                .filter(|mark| mark.kind() != kind)
                .cloned()
                .collect();
            piece.marks = MarkSet::new(remaining)?;
        }
    }

    rebuild(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LinkMark;

    fn inline(text: &str, marks: MarkSet) -> InlineContent {
        InlineContent::new([TextRun::new(text, marks).unwrap()]).unwrap()
    }

    fn range(start: usize, end: usize) -> TextRange {
        TextRange::new(offset_at(start), offset_at(end)).unwrap()
    }

    fn offset_at(raw: usize) -> crate::text::TextOffset {
        const SCRATCH: &str = "00000000000000000000000000000000";
        crate::text::TextBuffer::from(SCRATCH)
            .offset_at(raw)
            .unwrap()
    }

    #[test]
    fn replace_splices_across_runs_and_normalizes() {
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        let content = InlineContent::new([
            TextRun::new("你好", bold.clone()).unwrap(),
            TextRun::new("world", MarkSet::empty()).unwrap(),
        ])
        .unwrap();

        // Replace across the run boundary "好wor" → "XX".
        let next = replace_text(&content, range(3, 9), "XX").unwrap();
        let runs = next.runs();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text().as_str(), "你XX");
        assert_eq!(runs[0].marks(), &bold);
        assert_eq!(runs[1].text().as_str(), "ld");
    }

    #[test]
    fn replacement_inherits_marks_of_start_piece() {
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        let content = inline("abc", bold);
        let next = replace_text(&content, range(1, 2), "XYZ").unwrap();
        assert_eq!(next.runs().len(), 1);
        assert_eq!(next.runs()[0].text().as_str(), "aXYZc");
        assert!(next.runs()[0].marks().contains(MarkKind::Bold));
    }

    #[test]
    fn full_range_replacement_can_empty_content() {
        let content = inline("中文", MarkSet::empty());
        let next = replace_text(&content, range(0, 6), "").unwrap();
        assert!(next.is_empty());
    }

    #[test]
    fn add_mark_replaces_conflicting_same_kind() {
        let old_link = Mark::Link(LinkMark::new("https://old.example", None));
        let content = inline("abc", MarkSet::new([old_link.clone()]).unwrap());

        let next = add_mark(
            &content,
            range(0, 3),
            Mark::Link(LinkMark::new("https://new.example", None)),
        )
        .unwrap();

        assert_eq!(next.runs().len(), 1);
        let marks = next.runs()[0].marks();
        assert_eq!(
            marks.as_slice(),
            &[Mark::Link(LinkMark::new("https://new.example", None))]
        );
    }

    #[test]
    fn mark_ops_split_ranges_precisely() {
        let content = inline("abcd", MarkSet::empty());
        let next = add_mark(&content, range(1, 3), Mark::Bold).unwrap();

        assert_eq!(next.runs().len(), 3);
        assert!(!next.runs()[0].marks().contains(MarkKind::Bold));
        assert!(next.runs()[1].marks().contains(MarkKind::Bold));
        assert!(!next.runs()[2].marks().contains(MarkKind::Bold));

        let cleared = remove_mark(&next, range(0, 4), MarkKind::Bold).unwrap();
        assert!(cleared.runs()[0].marks().is_empty());
    }

    #[test]
    fn mid_code_point_ranges_are_rejected() {
        let cjk = inline("中文", MarkSet::empty());
        let mid = TextRange::new(offset_at(0), offset_at(1)).unwrap();

        assert!(replace_text(&cjk, mid, "x").is_err());
        assert!(add_mark(&cjk, mid, Mark::Bold).is_err());
        assert!(remove_mark(&cjk, mid, MarkKind::Bold).is_err());
    }
}
