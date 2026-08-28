//! Structured clipboard paste planning.
//!
//! A Xiaomu clipboard slice is compiled into ordinary Core transaction steps.
//! The first clipboard block merges into the target block at the selection
//! seam. With multiple source blocks, later blocks become fresh siblings and
//! the target block's unselected suffix moves to the final inserted block.
//! Source inline marks are restored explicitly so `ReplaceText` inheritance
//! cannot leak host formatting into pasted content.

use xiaomu_core::document::{InlineContent, MarkKind, NodeContent, NodeId, TextRun, XiaomuDocument};
use xiaomu_core::text::{TextBuffer, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionStep};

use crate::clipboard::{ClipboardBlock, ClipboardSlice};

use super::cross_block;
use super::intent::{EditPlan, PlannedAction, PrimaryEdit, SelectionUpdate, concatenated};
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

    let mut transaction = user_transaction();
    let (working, node, range) = prepare_target(document, selection, &mut transaction)?;
    let blocks = slice.blocks();

    if blocks.len() == 1 {
        let source = blocks[0].inline();
        let source_text = concatenated(source);
        if source_text.is_empty() && range.is_empty() && transaction.steps().is_empty() {
            return Ok(PlannedAction::NoChange);
        }
        plan_single_block(&working, node, range, source, &mut transaction)?;
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
    plan_multiple_blocks(&working, node, range, blocks, &mut transaction)?;
    Ok(PlannedAction::Commit(EditPlan::new(
        transaction,
        SelectionUpdate::CaretAtLastInsertedOffset {
            offset: last_pasted_len,
        },
        None,
    )))
}

/// Resolves the snapshot and empty/single-node range against which paste steps
/// are planned. Cross-block deletion is applied only to a temporary snapshot;
/// its steps are copied into the final transaction so no intermediate state
/// ever becomes visible to the session.
fn prepare_target(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    transaction: &mut Transaction,
) -> Result<(XiaomuDocument, NodeId, TextRange), SessionError> {
    if let Some(single) = selection.as_single_node() {
        let range = single
            .ordered_range()
            .map_err(|_| SessionError::SelectionInvalid)?;
        return Ok((document.clone(), single.focus().node_id(), range));
    }

    let (head, _) = selection.ordered(document)?;
    let DocumentPosition::Text(head) = head else {
        return Err(SessionError::SelectionInvalid);
    };
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
        .validate_offset(head.offset())
        .map_err(SessionError::Core)?;
    Ok((
        working,
        head.node_id(),
        TextRange::empty(head.offset()),
    ))
}

fn plan_single_block(
    document: &XiaomuDocument,
    node: NodeId,
    range: TextRange,
    source: &InlineContent,
    transaction: &mut Transaction,
) -> Result<(), SessionError> {
    let target = inline_of(document, node)?;
    validate_inline_range(target, range)?;
    let target_text = concatenated(target);
    let source_text = concatenated(source);
    let start = range.start().as_usize();
    let end = range.end().as_usize();
    let post_text = format!("{}{}{}", &target_text[..start], source_text, &target_text[end..]);

    transaction.push_step(TransactionStep::ReplaceText {
        node,
        range,
        replacement: concatenated(source),
    });
    push_exact_marks(transaction, node, source, start, &post_text)
}

fn plan_multiple_blocks(
    document: &XiaomuDocument,
    node: NodeId,
    range: TextRange,
    blocks: &[ClipboardBlock],
    transaction: &mut Transaction,
) -> Result<(), SessionError> {
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
    let first_text = concatenated(first.inline());
    let target_end = target
        .offset_at(target.len_bytes())
        .map_err(SessionError::Core)?;
    transaction.push_step(TransactionStep::ReplaceText {
        node,
        range: TextRange::new(range.start(), target_end).map_err(SessionError::Core)?,
        replacement: first_text.clone(),
    });
    let post_head = format!("{}{}", &target_text[..start], first_text);
    push_exact_marks(transaction, node, first.inline(), start, &post_head)?;

    for (offset, block) in blocks[1..blocks.len() - 1].iter().enumerate() {
        transaction.push_step(TransactionStep::InsertNode {
            parent,
            index: position + 1 + offset,
            kind: block.kind().clone(),
            attrs: block.attrs().clone(),
            content: NodeContent::Inline(block.inline().clone()),
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
    source: &InlineContent,
    inserted_start: usize,
    post_text: &str,
) -> Result<(), SessionError> {
    if source.is_empty() {
        return Ok(());
    }

    let buffer = TextBuffer::from_string(post_text.to_owned());
    let inserted_end = inserted_start + source.len_bytes();
    let whole = buffer
        .range(
            buffer.offset_at(inserted_start).map_err(SessionError::Core)?,
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
    for run in source.runs() {
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

fn inline_of(
    document: &XiaomuDocument,
    node: NodeId,
) -> Result<&InlineContent, SessionError> {
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
