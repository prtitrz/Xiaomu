//! Structured clipboard paste planning.
//!
//! Flat leaf-only slices merge through the target seam in one Core
//! transaction. Slices that retain container hierarchy are delegated to the
//! staged hierarchy planner so list/quote semantics survive reconstruction
//! while the whole paste remains one history entry.

use xiaomu_core::document::{
    InlineContent, MarkKind, NodeContent, NodeId, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::{TextBuffer, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionStep};

use crate::clipboard::{ClipboardBlock, ClipboardInline, ClipboardNodeContent, ClipboardSlice};

use super::atom_edit::atoms_inside_span;
use super::cross_block_atom as cross_block;
use super::intent::{EditPlan, PlannedAction, PrimaryEdit, SelectionUpdate, concatenated};
use super::paste_hierarchy;
use super::structure::{children_of, user_transaction};
use super::{DocumentPosition, DocumentSelection, SessionError};

const MARK_KINDS: [MarkKind; 6] = [
    MarkKind::Bold,
    MarkKind::Italic,
    MarkKind::Code,
    MarkKind::Underline,
    MarkKind::Strike,
    MarkKind::Link,
];

/// Plans one structured paste, including replacement of an existing
/// cross-block selection when necessary.
pub(crate) fn plan_paste_slice(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    slice: &ClipboardSlice,
) -> Result<PlannedAction, SessionError> {
    selection
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;
    if slice.blocks().is_empty() {
        return Ok(PlannedAction::NoChange);
    }

    if slice
        .roots()
        .iter()
        .any(|root| matches!(root.content(), ClipboardNodeContent::Children(_)))
    {
        return paste_hierarchy::plan_paste_hierarchy(document, selection, slice);
    }

    let mut transaction = user_transaction();
    let (working, node, start_gap, end_gap) =
        prepare_target(document, selection, &mut transaction)?;
    let blocks = slice.blocks();

    if blocks.len() == 1 {
        let source = blocks[0].inline();
        let source_text = source.text();
        if source.is_empty() && start_gap == end_gap && transaction.steps().is_empty() {
            return Ok(PlannedAction::NoChange);
        }
        plan_single_block(&working, node, start_gap, end_gap, source, &mut transaction)?;
        let range = TextRange::new(start_gap.text_offset(), end_gap.text_offset())
            .map_err(SessionError::Core)?;
        return Ok(PlannedAction::Commit(EditPlan::new(
            transaction,
            SelectionUpdate::CaretAfterReplacement,
            Some(PrimaryEdit {
                node,
                range,
                inserted_len: source_text.len(),
            }),
        )));
    }

    let last_pasted_len = blocks
        .last()
        .map(|block| block.inline().len_bytes())
        .ok_or(SessionError::SelectionInvalid)?;
    let range = TextRange::new(start_gap.text_offset(), end_gap.text_offset())
        .map_err(SessionError::Core)?;
    plan_multiple_blocks(&working, node, range, blocks, &mut transaction)?;
    Ok(PlannedAction::Commit(EditPlan::new(
        transaction,
        SelectionUpdate::CaretAtLastInsertedOffset {
            offset: last_pasted_len,
        },
        None,
    )))
}

/// Resolves the snapshot and mixed-inline replacement span against which
/// paste steps are planned. Cross-block deletion is applied only to a
/// temporary snapshot; its steps are copied into the final transaction so no
/// intermediate state ever becomes visible to the session.
fn prepare_target(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    transaction: &mut Transaction,
) -> Result<(XiaomuDocument, NodeId, InlinePoint, InlinePoint), SessionError> {
    let (head, tail) = selection.ordered(document)?;
    let (DocumentPosition::Inline(head), DocumentPosition::Inline(tail)) = (head, tail) else {
        return Err(SessionError::SelectionInvalid);
    };

    if head.node_id() == tail.node_id() {
        return Ok((document.clone(), head.node_id(), head, tail));
    }

    let action = cross_block::plan_delete_selection(document, selection)?;
    let PlannedAction::Commit(delete_plan) = action else {
        return Err(SessionError::SelectionInvalid);
    };
    for step in delete_plan.transaction().steps() {
        transaction.push_step(step.clone());
    }
    let working = delete_plan
        .transaction()
        .apply(document)
        .map_err(SessionError::Core)?;
    let inline = inline_of(&working, head.node_id())?;
    inline
        .validate_offset(head.text_offset())
        .map_err(SessionError::Core)?;
    Ok((working, head.node_id(), head, head))
}

fn plan_single_block(
    document: &XiaomuDocument,
    node: NodeId,
    start_gap: InlinePoint,
    end_gap: InlinePoint,
    source: &ClipboardInline,
    transaction: &mut Transaction,
) -> Result<(), SessionError> {
    let target = inline_of(document, node)?;
    validate_inline_range(target, text_range_of(start_gap, end_gap)?)?;
    let target_text = concatenated(target);
    let source_text = source.text();
    let start = start_gap.text_offset().as_usize();
    let end = end_gap.text_offset().as_usize();
    let post_text = format!(
        "{}{}{}",
        &target_text[..start],
        source_text,
        &target_text[end..]
    );
    let post_buffer = TextBuffer::from_string(post_text.clone());

    // Atoms inside the replaced target span are removed by identity; the
    // seam ordinal of `start_gap` already excludes them.
    for atom in atoms_inside_span(target, start_gap, end_gap) {
        transaction.push_step(TransactionStep::RemoveInlineAtom { atom });
    }
    transaction.push_step(TransactionStep::ReplaceInlineText {
        at: start_gap,
        end: end_gap.text_offset(),
        replacement: source_text.clone(),
    });
    push_exact_marks(transaction, node, source.runs(), start, &post_text)?;

    // Re-anchor the detached source atoms against the post-edit text. The
    // source anchor is relative to the inserted slice, so `start + anchor`
    // may lie beyond the pre-edit target length (for example a trailing atom
    // pasted at the target end). Validating against the old target would
    // reject otherwise valid pasted atoms.
    let mut previous_anchor: Option<usize> = None;
    let mut same_anchor_seen = 0usize;
    for atom in source.atoms() {
        let anchor = atom.anchor().as_usize();
        if previous_anchor == Some(anchor) {
            same_anchor_seen += 1;
        } else {
            same_anchor_seen = 0;
            previous_anchor = Some(anchor);
        }
        let ordinal = if anchor == 0 {
            start_gap.atom_index() + same_anchor_seen
        } else {
            same_anchor_seen
        };
        let at = InlinePoint::new(
            node,
            post_buffer
                .offset_at(start + anchor)
                .map_err(SessionError::Core)?,
            ordinal,
            start_gap.affinity(),
        );
        transaction.push_step(TransactionStep::InsertInlineAtom {
            at,
            kind: atom.kind().clone(),
            attrs: atom.attrs().clone(),
            content: atom.content().clone(),
        });
    }
    Ok(())
}

fn text_range_of(start_gap: InlinePoint, end_gap: InlinePoint) -> Result<TextRange, SessionError> {
    TextRange::new(start_gap.text_offset(), end_gap.text_offset()).map_err(SessionError::Core)
}

fn plan_multiple_blocks(
    document: &XiaomuDocument,
    node: NodeId,
    range: TextRange,
    blocks: &[ClipboardBlock],
    transaction: &mut Transaction,
) -> Result<(), SessionError> {
    if blocks
        .iter()
        .any(|block| !block.inline().atoms().is_empty())
    {
        // One declarative transaction cannot address the freshly allocated
        // blocks; fail closed instead of downgrading atoms to text.
        return Err(SessionError::ClipboardAtomsUnsupported);
    }
    let target = inline_of(document, node)?;
    validate_inline_range(target, range)?;
    let parent = document
        .parent_of(node)
        .ok_or(SessionError::SelectionInvalid)?;
    let position = children_of(document, parent)
        .iter()
        .position(|child| *child == node)
        .ok_or(SessionError::SelectionInvalid)?;

    let target_text = concatenated(target);
    let start = range.start().as_usize();
    let end = range.end().as_usize();
    let first = blocks.first().ok_or(SessionError::SelectionInvalid)?;
    let first_text = first.inline().text();
    let target_end = target
        .offset_at(target.len_bytes())
        .map_err(SessionError::Core)?;
    transaction.push_step(TransactionStep::ReplaceText {
        node,
        range: TextRange::new(range.start(), target_end).map_err(SessionError::Core)?,
        replacement: first_text.clone(),
    });
    let post_head = format!("{}{}", &target_text[..start], first_text);
    push_exact_marks(transaction, node, first.inline().runs(), start, &post_head)?;

    for (offset, block) in blocks[1..blocks.len() - 1].iter().enumerate() {
        transaction.push_step(TransactionStep::InsertNode {
            parent,
            index: position + 1 + offset,
            kind: block.kind().clone(),
            attrs: block.attrs().clone(),
            content: NodeContent::Inline(
                InlineContent::new(block.inline().runs().iter().cloned())
                    .map_err(SessionError::Core)?,
            ),
        });
    }

    let last = blocks.last().ok_or(SessionError::SelectionInvalid)?;
    let suffix = slice_inline(target, end, target.len_bytes())?;
    let last_inline = InlineContent::new(
        last.inline()
            .runs()
            .iter()
            .cloned()
            .chain(suffix.runs().iter().cloned()),
    )
    .map_err(SessionError::Core)?;
    transaction.push_step(TransactionStep::InsertNode {
        parent,
        index: position + blocks.len() - 1,
        kind: last.kind().clone(),
        attrs: last.attrs().clone(),
        content: NodeContent::Inline(last_inline),
    });
    Ok(())
}

/// Replaces inherited formatting over one newly inserted text span with the
/// exact run marks from the clipboard source.
fn push_exact_marks(
    transaction: &mut Transaction,
    node: NodeId,
    source_runs: &[TextRun],
    inserted_start: usize,
    post_text: &str,
) -> Result<(), SessionError> {
    if source_runs.is_empty() {
        return Ok(());
    }

    let buffer = TextBuffer::from_string(post_text.to_owned());
    let inserted_end = inserted_start + source_runs.iter().map(TextRun::len_bytes).sum::<usize>();
    let whole = buffer
        .range(
            buffer
                .offset_at(inserted_start)
                .map_err(SessionError::Core)?,
            buffer.offset_at(inserted_end).map_err(SessionError::Core)?,
        )
        .map_err(SessionError::Core)?;
    for kind in MARK_KINDS {
        transaction.push_step(TransactionStep::RemoveMark {
            node,
            range: whole,
            mark_kind: kind,
        });
    }

    let mut cursor = inserted_start;
    for run in source_runs {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;
        let run_range = buffer
            .range(
                buffer.offset_at(run_start).map_err(SessionError::Core)?,
                buffer.offset_at(run_end).map_err(SessionError::Core)?,
            )
            .map_err(SessionError::Core)?;
        for mark in run.marks().as_slice() {
            transaction.push_step(TransactionStep::AddMark {
                node,
                range: run_range,
                mark: mark.clone(),
            });
        }
    }
    Ok(())
}

fn validate_inline_range(inline: &InlineContent, range: TextRange) -> Result<(), SessionError> {
    inline
        .validate_offset(range.start())
        .map_err(SessionError::Core)?;
    inline
        .validate_offset(range.end())
        .map_err(SessionError::Core)?;
    Ok(())
}

fn inline_of(document: &XiaomuDocument, node: NodeId) -> Result<&InlineContent, SessionError> {
    document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)
}

fn slice_inline(
    inline: &InlineContent,
    start: usize,
    end: usize,
) -> Result<InlineContent, SessionError> {
    inline.offset_at(start).map_err(SessionError::Core)?;
    inline.offset_at(end).map_err(SessionError::Core)?;
    if start > end {
        return Err(SessionError::SelectionInvalid);
    }

    let mut runs = Vec::new();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;
        let overlap_start = start.max(run_start);
        let overlap_end = end.min(run_end);
        if overlap_start >= overlap_end {
            continue;
        }
        let text = &run.text().as_str()[overlap_start - run_start..overlap_end - run_start];
        runs.push(TextRun::new(text, run.marks().clone()).map_err(SessionError::Core)?);
    }
    InlineContent::new(runs).map_err(SessionError::Core)
}
