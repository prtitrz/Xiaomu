//! Atom-aware cross-block deletion.
//!
//! Core deliberately keeps `JoinNodes` fail-closed when either side carries
//! inline atoms because a text-only join cannot prove placement migration.
//! Runtime therefore decomposes a mixed-inline cross-block deletion into
//! explicit atom and text operations: selected head atoms are removed, the
//! tail's unselected text suffix is appended through `ReplaceInlineText`, and
//! unselected tail atoms are moved by identity with
//! `RemoveInlineAtom -> RestoreInlineAtom` before the tail block is removed.

use std::collections::BTreeSet;

use xiaomu_core::document::{
    InlineContent, MarkKind, Node, NodeContent, NodeId, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::TransactionStep;

use super::cross_block;
use super::intent::{EditPlan, PlannedAction, PrimaryEdit, SelectionUpdate};
use super::structure::user_transaction;
use super::{DocumentPosition, DocumentSelection, SessionError};

const MARK_KINDS: [MarkKind; 6] = [
    MarkKind::Bold,
    MarkKind::Italic,
    MarkKind::Code,
    MarkKind::Underline,
    MarkKind::Strike,
    MarkKind::Link,
];

/// Uses the established plain-text planner when both boundary blocks are
/// atom-free, otherwise applies the mixed-inline migration contract below.
pub(crate) fn plan_delete_selection(
    document: &XiaomuDocument,
    selection: DocumentSelection,
) -> Result<PlannedAction, SessionError> {
    selection
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;
    let (head, tail) = selection.ordered(document)?;
    let (DocumentPosition::Inline(head), DocumentPosition::Inline(tail)) = (head, tail) else {
        return Err(SessionError::SelectionInvalid);
    };
    if head.node_id() == tail.node_id() {
        return Err(SessionError::SelectionInvalid);
    }

    let head_inline = inline_of(document, head.node_id())?;
    let tail_inline = inline_of(document, tail.node_id())?;
    if head_inline.atoms().is_empty() && tail_inline.atoms().is_empty() {
        return cross_block::plan_delete_selection(document, selection);
    }

    plan_mixed_delete(document, head, tail, head_inline, tail_inline)
}

fn plan_mixed_delete(
    document: &XiaomuDocument,
    head: InlinePoint,
    tail: InlinePoint,
    head_inline: &InlineContent,
    tail_inline: &InlineContent,
) -> Result<PlannedAction, SessionError> {
    let blocks = inline_blocks(document);
    let head_index = blocks
        .iter()
        .position(|node| *node == head.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    let tail_index = blocks
        .iter()
        .position(|node| *node == tail.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    if head_index >= tail_index {
        return Err(SessionError::SelectionInvalid);
    }

    head_inline
        .validate_offset(head.text_offset())
        .map_err(SessionError::Core)?;
    tail_inline
        .validate_offset(tail.text_offset())
        .map_err(SessionError::Core)?;

    let head_text = concatenated(head_inline);
    let head_start = head.text_offset().as_usize();
    let head_end = head_inline.len_bytes();
    let tail_start = tail.text_offset().as_usize();
    let tail_end = tail_inline.len_bytes();
    let tail_suffix_runs = slice_runs(tail_inline, tail_start, tail_end)?;
    let tail_suffix_text: String = tail_suffix_runs
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    let post_head_text = format!("{}{}", &head_text[..head_start], tail_suffix_text);
    let post_head_buffer = TextBuffer::from_string(post_head_text.clone());

    let mut transaction = user_transaction();

    // Every head atom from the selected start gap through the end of the
    // block belongs to the cross-block selection, including trailing atoms at
    // the text-end boundary. Remove them explicitly before the text edit.
    for atom in atoms_from_gap_to_end(head_inline, head) {
        transaction.push_step(TransactionStep::RemoveInlineAtom { atom });
    }

    transaction.push_step(TransactionStep::ReplaceInlineText {
        at: head,
        end: head_inline
            .offset_at(head_end)
            .map_err(SessionError::Core)?,
        replacement: tail_suffix_text.clone(),
    });
    push_exact_marks(
        &mut transaction,
        head.node_id(),
        &tail_suffix_runs,
        head_start,
        &post_head_text,
    )?;

    // Tail atoms at/after the tail selection gap are unselected suffix
    // content. Move them under the surviving head with their exact identity,
    // payload and canonical same-boundary order. Selected prefix atoms remain
    // under tail and disappear with the removed tail subtree.
    for preserved in suffix_atoms(document, tail_inline, tail)? {
        let relative = preserved.offset.as_usize() - tail_start;
        let destination_offset = post_head_buffer
            .offset_at(head_start + relative)
            .map_err(SessionError::Core)?;
        let destination_ordinal = if preserved.offset == tail.text_offset() {
            head.atom_index() + preserved.ordinal - tail.atom_index()
        } else {
            preserved.ordinal
        };
        transaction.push_step(TransactionStep::RemoveInlineAtom {
            atom: preserved.node.id(),
        });
        transaction.push_step(TransactionStep::RestoreInlineAtom {
            at: InlinePoint::new(
                head.node_id(),
                destination_offset,
                destination_ordinal,
                head.affinity(),
            ),
            node: preserved.node,
        });
    }

    transaction.push_step(TransactionStep::RemoveNode {
        node: tail.node_id(),
    });

    let middle: Vec<NodeId> = blocks[head_index + 1..tail_index].to_vec();
    for node in &middle {
        transaction.push_step(TransactionStep::RemoveNode { node: *node });
    }

    let removed_from_original: BTreeSet<_> = middle
        .iter()
        .copied()
        .chain(std::iter::once(tail.node_id()))
        .collect();
    for container in empty_container_roots(document, &removed_from_original) {
        transaction.push_step(TransactionStep::RemoveNode { node: container });
    }

    let caret_range = TextRange::empty(head.text_offset());
    Ok(PlannedAction::Commit(EditPlan::new(
        transaction,
        SelectionUpdate::CaretAtInline { caret: head },
        Some(PrimaryEdit {
            node: head.node_id(),
            range: caret_range,
            inserted_len: 0,
        }),
    )))
}

struct PreservedAtom {
    node: Node,
    offset: TextOffset,
    ordinal: usize,
}

fn suffix_atoms(
    document: &XiaomuDocument,
    inline: &InlineContent,
    tail: InlinePoint,
) -> Result<Vec<PreservedAtom>, SessionError> {
    let start = tail.text_offset().as_usize();
    let mut atoms = Vec::new();
    for placement in inline.atoms() {
        let raw = placement.text_offset().as_usize();
        let ordinal = same_boundary_ordinal(inline, placement.text_offset(), placement.atom());
        let preserved = raw > start || (raw == start && ordinal >= tail.atom_index());
        if !preserved {
            continue;
        }
        let node = document
            .node(placement.atom())
            .cloned()
            .ok_or(SessionError::SelectionInvalid)?;
        atoms.push(PreservedAtom {
            node,
            offset: placement.text_offset(),
            ordinal,
        });
    }
    Ok(atoms)
}

fn atoms_from_gap_to_end(inline: &InlineContent, head: InlinePoint) -> Vec<NodeId> {
    let start = head.text_offset().as_usize();
    inline
        .atoms()
        .iter()
        .filter_map(|placement| {
            let raw = placement.text_offset().as_usize();
            let ordinal = same_boundary_ordinal(inline, placement.text_offset(), placement.atom());
            (raw > start || (raw == start && ordinal >= head.atom_index()))
                .then_some(placement.atom())
        })
        .collect()
}

fn same_boundary_ordinal(inline: &InlineContent, offset: TextOffset, atom: NodeId) -> usize {
    inline
        .atoms()
        .iter()
        .filter(|placement| placement.text_offset() == offset)
        .take_while(|placement| placement.atom() != atom)
        .count()
}

fn slice_runs(
    inline: &InlineContent,
    start: usize,
    end: usize,
) -> Result<Vec<TextRun>, SessionError> {
    inline.offset_at(start).map_err(SessionError::Core)?;
    inline.offset_at(end).map_err(SessionError::Core)?;
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
    Ok(runs)
}

fn push_exact_marks(
    transaction: &mut xiaomu_core::transaction::Transaction,
    node: NodeId,
    source_runs: &[TextRun],
    inserted_start: usize,
    post_text: &str,
) -> Result<(), SessionError> {
    if source_runs.is_empty() {
        return Ok(());
    }
    let buffer = TextBuffer::from_string(post_text.to_owned());
    let inserted_len: usize = source_runs.iter().map(TextRun::len_bytes).sum();
    let whole = TextRange::new(
        buffer
            .offset_at(inserted_start)
            .map_err(SessionError::Core)?,
        buffer
            .offset_at(inserted_start + inserted_len)
            .map_err(SessionError::Core)?,
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
        let range = TextRange::new(
            buffer.offset_at(run_start).map_err(SessionError::Core)?,
            buffer.offset_at(run_end).map_err(SessionError::Core)?,
        )
        .map_err(SessionError::Core)?;
        for mark in run.marks().as_slice() {
            transaction.push_step(TransactionStep::AddMark {
                node,
                range,
                mark: mark.clone(),
            });
        }
    }
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

fn concatenated(inline: &InlineContent) -> String {
    inline
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn inline_blocks(document: &XiaomuDocument) -> Vec<NodeId> {
    fn walk(document: &XiaomuDocument, id: NodeId, out: &mut Vec<NodeId>) {
        let Some(node) = document.node(id) else {
            return;
        };
        match node.content() {
            NodeContent::Inline(_) => out.push(id),
            NodeContent::Children(children) => {
                for child in children {
                    walk(document, *child, out);
                }
            }
            NodeContent::Atomic | _ => {}
        }
    }

    let mut blocks = Vec::new();
    walk(document, document.root(), &mut blocks);
    blocks
}

fn empty_container_roots(document: &XiaomuDocument, removed: &BTreeSet<NodeId>) -> Vec<NodeId> {
    fn analyze(
        document: &XiaomuDocument,
        id: NodeId,
        removed: &BTreeSet<NodeId>,
        candidates: &mut BTreeSet<NodeId>,
    ) -> (bool, bool) {
        if removed.contains(&id) {
            return (true, true);
        }
        let Some(node) = document.node(id) else {
            return (false, false);
        };
        let NodeContent::Children(children) = node.content() else {
            return (false, false);
        };

        let mut affected = false;
        let mut all_removed = !children.is_empty();
        for child in children {
            let (child_removed, child_affected) = analyze(document, *child, removed, candidates);
            affected |= child_affected;
            all_removed &= child_removed;
        }

        let removable = id != document.root() && affected && all_removed;
        if removable {
            candidates.insert(id);
        }
        (removable, affected)
    }

    let mut candidates = BTreeSet::new();
    analyze(document, document.root(), removed, &mut candidates);

    candidates
        .iter()
        .copied()
        .filter(|candidate| {
            let mut parent = document.parent_of(*candidate);
            while let Some(id) = parent {
                if candidates.contains(&id) {
                    return false;
                }
                parent = document.parent_of(id);
            }
            true
        })
        .collect()
}
