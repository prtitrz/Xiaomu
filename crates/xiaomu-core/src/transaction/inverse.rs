//! Inverse step generation for applied transactions.
//!
//! The applying engine records inverse steps while it still sees the
//! before-state of each step. Inverse groups are emitted per step and later
//! reversed by the caller, so each group's coordinates match the intermediate
//! state the original step produced.

use crate::Result;
use crate::document::{InlineContent, MarkKind, MarkSet, NodeId};
use crate::text::{TextOffset, TextRange};

use super::step::TransactionStep;

/// One maximal same-mark span of inline text with its absolute range.
pub(super) struct InlineSpan {
    pub(super) range: TextRange,
    pub(super) marks: MarkSet,
    pub(super) text: String,
}

/// Returns the maximal same-mark spans of `inline` that lie inside `range`,
/// with absolute coordinates and the exact covered text.
///
/// This also validates the range like the inline edit helpers do, so callers
/// invoke it before mutating anything.
pub(super) fn spans_within(inline: &InlineContent, range: TextRange) -> Result<Vec<InlineSpan>> {
    if range.start() > range.end() {
        return Err(crate::Error::InvalidTextRange {
            start: range.start().as_usize(),
            end: range.end().as_usize(),
        });
    }
    inline.validate_offset(range.start())?;
    inline.validate_offset(range.end())?;

    let start = range.start().as_usize();
    let end = range.end().as_usize();

    let mut spans = Vec::new();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;

        let span_start = start.max(run_start);
        let span_end = end.min(run_end);
        if span_start < span_end {
            spans.push(InlineSpan {
                range: TextRange::new(
                    TextOffset::from_validated_byte_index(span_start),
                    TextOffset::from_validated_byte_index(span_end),
                )?,
                marks: run.marks().clone(),
                text: run.text().as_str()[span_start - run_start..span_end - run_start].to_owned(),
            });
        }
    }

    Ok(spans)
}

/// Returns the mark set that a replacement starting at `offset` would
/// inherit under `replace_text`'s rules.
///
/// A replacement inherits the marks of the first run whose end reaches the
/// start offset, so a start exactly at a run boundary resolves to the
/// preceding run; offset zero resolves to the first run and an insertion at
/// the very end resolves to the last run.
fn inherited_marks_at(inline: &InlineContent, offset: usize) -> MarkSet {
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_end = cursor + run.len_bytes();
        if offset <= run_end {
            return run.marks().clone();
        }
        cursor = run_end;
    }

    MarkSet::empty()
}

fn offset(raw: usize) -> TextOffset {
    TextOffset::from_validated_byte_index(raw)
}

/// Builds the inverse steps of one applied `ReplaceText`.
///
/// The inverse restores the old text, strips the marks the replacement had
/// inherited from the restored span, and re-adds each old span's marks so
/// even replacements crossing differently-marked runs round-trip exactly.
pub(super) fn replace_text_inverse(
    node: NodeId,
    range: TextRange,
    replacement: &str,
    pre: &InlineContent,
    spans: &[InlineSpan],
) -> Vec<TransactionStep> {
    let start = range.start().as_usize();

    if spans.is_empty() {
        // Empty range: a pure insertion, and deleting the inserted bytes is
        // the exact inverse.
        return vec![TransactionStep::ReplaceText {
            node,
            range: TextRange::new(offset(start), offset(start + replacement.len()))
                .expect("inverse range stays ordered"),
            replacement: String::new(),
        }];
    }

    let old_text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let old_len = old_text.len();
    let mut steps = vec![TransactionStep::ReplaceText {
        node,
        range: TextRange::new(offset(start), offset(start + replacement.len()))
            .expect("inverse range stays ordered"),
        replacement: old_text,
    }];

    // After the restoring replacement, the restored span carries exactly the
    // marks the original replacement had inherited. Strip them so the
    // per-span re-add below reconstructs the original segmentation.
    let restored_span =
        TextRange::new(offset(start), offset(start + old_len)).expect("inverse span stays ordered");
    for mark in inherited_marks_at(pre, start).as_slice() {
        steps.push(TransactionStep::RemoveMark {
            node,
            range: restored_span,
            mark_kind: mark.kind(),
        });
    }

    for span in spans {
        for mark in span.marks.as_slice() {
            steps.push(TransactionStep::AddMark {
                node,
                range: span.range,
                mark: mark.clone(),
            });
        }
    }

    steps
}

/// Builds the inverse steps of one applied `AddMark`.
///
/// The added mark kind is stripped from the whole range, then each old span
/// that carried a mark of the same kind gets its original value back.
pub(super) fn add_mark_inverse(
    node: NodeId,
    range: TextRange,
    kind: MarkKind,
    spans: &[InlineSpan],
) -> Vec<TransactionStep> {
    if range.is_empty() {
        return Vec::new();
    }

    let mut steps = vec![TransactionStep::RemoveMark {
        node,
        range,
        mark_kind: kind,
    }];
    steps.extend(revive_marks(node, kind, spans));
    steps
}

/// Builds the inverse steps of one applied `RemoveMark`.
///
/// Each old span that carried a mark of the removed kind gets it back.
pub(super) fn remove_mark_inverse(
    node: NodeId,
    kind: MarkKind,
    spans: &[InlineSpan],
) -> Vec<TransactionStep> {
    revive_marks(node, kind, spans).collect()
}

/// Produces re-add steps for every old span carrying a mark of `kind`.
fn revive_marks(
    node: NodeId,
    kind: MarkKind,
    spans: &[InlineSpan],
) -> impl Iterator<Item = TransactionStep> + '_ {
    spans.iter().filter_map(move |span| {
        span.marks
            .as_slice()
            .iter()
            .find(|mark| mark.kind() == kind)
            .map(|mark| TransactionStep::AddMark {
                node,
                range: span.range,
                mark: mark.clone(),
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LinkMark, Mark, MarkSet, TextRun};

    fn inline(parts: &[(&str, MarkSet)]) -> InlineContent {
        let runs = parts
            .iter()
            .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap())
            .collect::<Vec<_>>();
        InlineContent::new(runs).unwrap()
    }

    fn offset_at(raw: usize) -> TextOffset {
        const SCRATCH: &str = "0000000000000000000000000000000000000000";
        crate::text::TextBuffer::from(SCRATCH)
            .offset_at(raw)
            .unwrap()
    }

    fn range(start: usize, end: usize) -> TextRange {
        TextRange::new(offset_at(start), offset_at(end)).unwrap()
    }

    #[test]
    fn spans_within_splits_at_run_boundaries() {
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        let content = inline(&[("你好", bold.clone()), ("world", MarkSet::empty())]);

        let spans = spans_within(&content, range(3, 9)).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "好");
        assert_eq!(spans[0].marks, bold);
        assert_eq!(spans[1].text, "wor");
        assert_eq!(spans[0].range, range(3, 6));
        assert_eq!(spans[1].range, range(6, 9));

        assert!(spans_within(&content, range(0, 1)).is_err());
    }

    #[test]
    fn marks_at_resolves_containing_and_last_runs() {
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        let link = MarkSet::new([Mark::Link(LinkMark::new("https://x.example", None))]).unwrap();
        let content = inline(&[("ab", bold.clone()), ("cd", link.clone())]);

        // Boundary offsets resolve to the preceding run, matching the
        // inheritance rules of `replace_text`.
        assert_eq!(inherited_marks_at(&content, 0), bold);
        assert_eq!(inherited_marks_at(&content, 2), bold);
        assert_eq!(inherited_marks_at(&content, 3), link);
        assert_eq!(inherited_marks_at(&content, 4), link);
        assert_eq!(
            inherited_marks_at(&InlineContent::empty(), 0),
            MarkSet::empty()
        );
    }
}
