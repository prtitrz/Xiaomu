//! Piece-based editing over mixed inline content.
//!
//! Run segmentation is an internal detail: these helpers split runs at range
//! boundaries, edit the affected pieces, and rebuild normalized
//! `InlineContent`. User-visible text coordinates stay UTF-8 document offsets,
//! while atom placements are preserved explicitly.

use crate::document::{InlineAtomPlacement, InlineContent, Mark, MarkKind, MarkSet, TextRun};
use crate::text::{TextOffset, TextRange};
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

/// Old text-only `ReplaceText` cannot address a position inside a same-byte
/// atom seam. Reject any edit whose closed boundary span contains an atom;
/// atom-aware editing uses `InlinePoint` in the dedicated P4 contract.
fn validate_text_replacement_against_atoms(inline: &InlineContent, range: TextRange) -> Result<()> {
    let start = range.start().as_usize();
    let end = range.end().as_usize();
    if inline.atoms().iter().any(|placement| {
        let offset = placement.text_offset().as_usize();
        offset >= start && offset <= end
    }) {
        return Err(Error::InvalidTransaction);
    }
    Ok(())
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

        // Untouched suffix after the affected span. A run that starts at or
        // after `end` is entirely suffix; slicing from `end - run_start`
        // would underflow for such runs.
        let suffix_start = end.max(run_start);
        if run_end > suffix_start {
            pieces.push(Piece::new(
                marks,
                run_text[suffix_start - run_start..].to_owned(),
            ));
        }
    }

    Ok(pieces)
}

fn rebuild(pieces: Vec<Piece>, atoms: &[InlineAtomPlacement]) -> Result<InlineContent> {
    let mut runs = Vec::new();
    for piece in pieces {
        if !piece.text.is_empty() {
            runs.push(TextRun::new(piece.text, piece.marks)?);
        }
    }
    InlineContent::with_atoms(runs, atoms.iter().copied())
}

fn mapped_atom_placements_after_text_replace(
    inline: &InlineContent,
    range: TextRange,
    replacement_len: usize,
) -> Vec<InlineAtomPlacement> {
    let start = range.start().as_usize();
    let end = range.end().as_usize();
    let removed_len = end - start;

    inline
        .atoms()
        .iter()
        .copied()
        .map(|placement| {
            let old = placement.text_offset().as_usize();
            if old > end {
                InlineAtomPlacement::new(
                    placement.atom(),
                    TextOffset::from_validated_byte_index(old - removed_len + replacement_len),
                )
            } else {
                placement
            }
        })
        .collect()
}

/// Returns inline content with `range` replaced by `replacement`.
///
/// The replacement inherits the marks of the piece containing
/// `range.start`, keeping continuous typing behavior deterministic. This
/// legacy text-only operation fails closed when an atom exists at either
/// endpoint or inside the range because only an [`crate::selection::InlinePoint`]
/// can distinguish the canonical gaps at that seam.
pub fn replace_text(
    inline: &InlineContent,
    range: TextRange,
    replacement: &str,
) -> Result<InlineContent> {
    validate_inline_range(inline, range)?;
    validate_text_replacement_against_atoms(inline, range)?;

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

    let atoms = mapped_atom_placements_after_text_replace(inline, range, replacement.len());
    rebuild(output, &atoms)
}

/// Returns inline content with `mark` applied to `range`.
///
/// An existing mark of the same kind inside the range is replaced so a run
/// never carries two competing values for one semantic mark. Atom placements
/// are preserved exactly because mark edits do not move text coordinates.
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

    rebuild(pieces, inline.atoms())
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

    rebuild(pieces, inline.atoms())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LinkMark, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};

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

    fn distinct_node_ids(count: usize) -> Vec<crate::document::NodeId> {
        let mut builder = NodeStoreBuilder::new();
        (0..count)
            .map(|_| {
                builder
                    .insert(
                        NodeKind::Paragraph,
                        NodeAttrs::empty(),
                        NodeContent::empty_inline(),
                    )
                    .unwrap()
            })
            .collect()
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
    fn replacement_shifts_later_atom_placements_without_fake_bytes() {
        let atom = distinct_node_ids(1)[0];
        let content = InlineContent::with_atoms(
            [TextRun::new("abcd", MarkSet::empty()).unwrap()],
            [InlineAtomPlacement::new(atom, offset_at(3))],
        )
        .unwrap();

        let next = replace_text(&content, range(0, 1), "XX").unwrap();
        assert_eq!(next.len_bytes(), 5);
        assert_eq!(next.atoms().len(), 1);
        assert_eq!(next.atoms()[0].atom(), atom);
        assert_eq!(next.atoms()[0].text_offset(), offset_at(4));
    }

    #[test]
    fn legacy_text_replacement_fails_closed_at_atom_seam() {
        let atom = distinct_node_ids(1)[0];
        let content = InlineContent::with_atoms(
            [TextRun::new("ab", MarkSet::empty()).unwrap()],
            [InlineAtomPlacement::new(atom, offset_at(1))],
        )
        .unwrap();

        assert_eq!(
            replace_text(&content, range(1, 1), "x"),
            Err(Error::InvalidTransaction)
        );
        assert_eq!(
            replace_text(&content, range(0, 1), "x"),
            Err(Error::InvalidTransaction)
        );
        assert_eq!(
            replace_text(&content, range(1, 2), "x"),
            Err(Error::InvalidTransaction)
        );
        assert_eq!(
            replace_text(&content, range(0, 2), "x"),
            Err(Error::InvalidTransaction)
        );
    }

    #[test]
    fn mark_ops_preserve_atom_placements() {
        let atom = distinct_node_ids(1)[0];
        let content = InlineContent::with_atoms(
            [TextRun::new("abcd", MarkSet::empty()).unwrap()],
            [InlineAtomPlacement::new(atom, offset_at(2))],
        )
        .unwrap();

        let marked = add_mark(&content, range(1, 3), Mark::Bold).unwrap();
        assert_eq!(marked.atoms(), content.atoms());
        let cleared = remove_mark(&marked, range(0, 4), MarkKind::Bold).unwrap();
        assert_eq!(cleared.atoms(), content.atoms());
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
    fn mark_ops_leave_later_runs_untouched() {
        // Regression: ranges that end before the last run must not slice the
        // suffix with an underflowing index.
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        let italic = MarkSet::new([Mark::Italic]).unwrap();
        let content = InlineContent::new([
            TextRun::new("你好", bold.clone()).unwrap(),
            TextRun::new("world", MarkSet::empty()).unwrap(),
            TextRun::new("tail", italic).unwrap(),
        ])
        .unwrap();

        let next = add_mark(&content, range(0, 3), Mark::Code).unwrap();
        assert_eq!(next.runs().len(), 4);
        assert_eq!(next.runs()[0].text().as_str(), "你");
        assert!(next.runs()[0].marks().contains(MarkKind::Code));
        assert_eq!(next.runs()[1].text().as_str(), "好");
        assert_eq!(next.runs()[1].marks(), &bold);
        assert_eq!(next.runs()[2].text().as_str(), "world");
        assert_eq!(next.runs()[3].text().as_str(), "tail");

        let cleared = remove_mark(&content, range(0, 3), MarkKind::Bold).unwrap();
        assert_eq!(cleared.runs().len(), 4);
        assert_eq!(cleared.runs()[0].text().as_str(), "你");
        assert!(cleared.runs()[0].marks().is_empty());
        assert!(cleared.runs()[1].marks().contains(MarkKind::Bold));
        assert_eq!(cleared.runs()[3].text().as_str(), "tail");
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
