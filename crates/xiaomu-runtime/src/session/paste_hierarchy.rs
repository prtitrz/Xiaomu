//! Hierarchical structured clipboard paste planning.
//!
//! Container fragments cannot be reconstructed in one Core transaction today:
//! `InsertNode` allocates a fresh identity, while later child inserts need that
//! identity as their parent. Runtime therefore uses the existing staged-command
//! seam. Intermediate snapshots never escape and the whole paste still records
//! exactly one history entry.

use xiaomu_core::document::{
    InlineContent, Node, NodeContent, NodeId, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{Transaction, TransactionStep};

use crate::clipboard::{ClipboardInline, ClipboardNode, ClipboardNodeContent, ClipboardSlice};

use super::atom_edit;
use super::cross_block_atom;
use super::intent::{HistoryPolicy, PlannedAction, SelectionUpdate};
use super::structure::{StagedPlan, children_of, user_transaction};
use super::{DocumentPosition, DocumentSelection, SessionError};

#[derive(Clone)]
struct FragmentLocation {
    root_index: usize,
    child_path: Vec<usize>,
}

/// Reconstructs a clipboard fragment tree between the target block's prefix
/// and suffix. Target deletion, atom-aware splitting, fragment insertion, and
/// detached atom materialization all run as hidden stages under one history
/// entry. Core can therefore keep `SplitNode` fail-closed for atom-bearing
/// blocks while Runtime owns the mixed-inline migration semantics.
pub(crate) fn plan_paste_hierarchy(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    slice: &ClipboardSlice,
) -> Result<PlannedAction, SessionError> {
    let last_offset = slice
        .blocks()
        .last()
        .map(|block| block.inline().len_bytes())
        .ok_or(SessionError::SelectionInvalid)?;
    let (target, split, deletion) = prepare_target(document, selection)?;

    let mut staged = StagedPlan::new(SelectionUpdate::CaretAtLastInsertedOffset {
        offset: last_offset,
    });
    if let Some(deletion) = deletion {
        staged = staged.stage(move |_| Ok(deletion));
    }
    staged = append_target_split_stages(staged, target, split);

    for (root_index, root) in slice.roots().iter().cloned().enumerate() {
        staged = append_node_stages(
            staged,
            target,
            FragmentLocation {
                root_index,
                child_path: Vec::new(),
            },
            root,
        )?;
    }

    Ok(PlannedAction::CommitStaged(staged))
}

/// Resolves the surviving target block, its final split gap, and an optional
/// deletion transaction. Same-block mixed selections use the atom-aware text
/// planner; cross-block selections use the atom-aware cross-block planner.
fn prepare_target(
    document: &XiaomuDocument,
    selection: DocumentSelection,
) -> Result<(NodeId, InlinePoint, Option<Transaction>), SessionError> {
    selection
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;
    let (head, tail) = selection.ordered(document)?;
    let (DocumentPosition::Inline(head), DocumentPosition::Inline(tail)) = (head, tail) else {
        return Err(SessionError::SelectionInvalid);
    };

    if head.node_id() == tail.node_id() {
        if same_gap(head, tail) {
            return Ok((head.node_id(), head, None));
        }
        let inline = inline_of(document, head.node_id())?;
        let action = atom_edit::plan_text_input(
            inline,
            Some(head),
            tail,
            "",
            None,
            HistoryPolicy::Isolated,
        )?;
        let PlannedAction::Commit(plan) = action else {
            return Err(SessionError::SelectionInvalid);
        };
        return Ok((head.node_id(), head, Some(plan.transaction().clone())));
    }

    let action = cross_block_atom::plan_delete_selection(document, selection)?;
    let PlannedAction::Commit(plan) = action else {
        return Err(SessionError::SelectionInvalid);
    };
    Ok((head.node_id(), head, Some(plan.transaction().clone())))
}

fn same_gap(first: InlinePoint, second: InlinePoint) -> bool {
    first.node_id() == second.node_id()
        && first.text_offset() == second.text_offset()
        && first.atom_index() == second.atom_index()
}

/// Splits `target` around one mixed-inline gap without using Core `SplitNode`.
///
/// Stage 1 inserts a text-only suffix sibling using the exact suffix runs and
/// marks. Stage 2 moves every atom on the suffix side by stable identity, then
/// truncates the surviving target through `ReplaceInlineText`. The seam's
/// `atom_index` decides which same-boundary atoms remain on each side.
fn append_target_split_stages(
    mut staged: StagedPlan,
    target: NodeId,
    split: InlinePoint,
) -> StagedPlan {
    let insert_split = split;
    staged = staged.stage(move |document| {
        insert_split
            .validate(document)
            .map_err(SessionError::Core)?;
        let node = document
            .node(target)
            .ok_or(SessionError::SelectionInvalid)?;
        let inline = node
            .content()
            .as_inline()
            .ok_or(SessionError::SelectionInvalid)?;
        let suffix_runs = slice_runs(
            inline,
            insert_split.text_offset().as_usize(),
            inline.len_bytes(),
        )?;
        let parent = document
            .parent_of(target)
            .ok_or(SessionError::SelectionInvalid)?;
        let index = children_of(document, parent)
            .iter()
            .position(|child| *child == target)
            .ok_or(SessionError::SelectionInvalid)?;
        Ok(user_transaction().with_step(TransactionStep::InsertNode {
            parent,
            index: index + 1,
            kind: node.kind().clone(),
            attrs: node.attrs().clone(),
            content: NodeContent::Inline(
                InlineContent::new(suffix_runs).map_err(SessionError::Core)?,
            ),
        }))
    });

    let migrate_split = split;
    staged.stage(move |document| {
        migrate_split
            .validate(document)
            .map_err(SessionError::Core)?;
        let parent = document
            .parent_of(target)
            .ok_or(SessionError::SelectionInvalid)?;
        let siblings = children_of(document, parent);
        let target_index = siblings
            .iter()
            .position(|child| *child == target)
            .ok_or(SessionError::SelectionInvalid)?;
        let suffix = *siblings
            .get(target_index + 1)
            .ok_or(SessionError::SelectionInvalid)?;
        let target_inline = inline_of(document, target)?;
        let suffix_inline = inline_of(document, suffix)?;
        let split_raw = migrate_split.text_offset().as_usize();
        let atoms = suffix_atoms(document, target_inline, migrate_split)?;

        let mut transaction = user_transaction();
        for atom in atoms {
            let relative = atom.offset.as_usize() - split_raw;
            let destination = suffix_inline
                .offset_at(relative)
                .map_err(SessionError::Core)?;
            let ordinal = if atom.offset == migrate_split.text_offset() {
                atom.ordinal - migrate_split.atom_index()
            } else {
                atom.ordinal
            };
            transaction.push_step(TransactionStep::RemoveInlineAtom {
                atom: atom.node.id(),
            });
            transaction.push_step(TransactionStep::RestoreInlineAtom {
                at: InlinePoint::new(
                    suffix,
                    destination,
                    ordinal,
                    migrate_split.affinity(),
                ),
                node: atom.node,
            });
        }

        let end = target_inline
            .offset_at(target_inline.len_bytes())
            .map_err(SessionError::Core)?;
        transaction.push_step(TransactionStep::ReplaceInlineText {
            at: migrate_split,
            end,
            replacement: String::new(),
        });
        Ok(transaction)
    })
}

struct PreservedAtom {
    node: Node,
    offset: TextOffset,
    ordinal: usize,
}

fn suffix_atoms(
    document: &XiaomuDocument,
    inline: &InlineContent,
    split: InlinePoint,
) -> Result<Vec<PreservedAtom>, SessionError> {
    let split_raw = split.text_offset().as_usize();
    let mut atoms = Vec::new();
    for placement in inline.atoms() {
        let raw = placement.text_offset().as_usize();
        let ordinal = same_boundary_ordinal(inline, placement.text_offset(), placement.atom());
        if raw < split_raw || (raw == split_raw && ordinal < split.atom_index()) {
            continue;
        }
        atoms.push(PreservedAtom {
            node: document
                .node(placement.atom())
                .cloned()
                .ok_or(SessionError::SelectionInvalid)?,
            offset: placement.text_offset(),
            ordinal,
        });
    }
    Ok(atoms)
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
    Ok(runs)
}

fn append_node_stages(
    mut staged: StagedPlan,
    target: NodeId,
    location: FragmentLocation,
    node: ClipboardNode,
) -> Result<StagedPlan, SessionError> {
    let insert_location = location.clone();
    let kind = node.kind().clone();
    let attrs = node.attrs().clone();
    let inline = match node.content() {
        ClipboardNodeContent::Inline(inline) => Some(inline.clone()),
        ClipboardNodeContent::Children(_) => None,
    };
    let children = match node.content() {
        ClipboardNodeContent::Children(children) => Some(children.clone()),
        ClipboardNodeContent::Inline(_) => None,
    };
    let content = match &inline {
        Some(inline) => NodeContent::Inline(
            InlineContent::new(inline.runs().iter().cloned()).map_err(SessionError::Core)?,
        ),
        None => NodeContent::children([]),
    };

    staged = staged.stage(move |document| {
        let (parent, index) = insertion_point(document, target, &insert_location)?;
        Ok(user_transaction().with_step(TransactionStep::InsertNode {
            parent,
            index,
            kind,
            attrs,
            content,
        }))
    });

    if let Some(inline) = inline
        && !inline.atoms().is_empty()
    {
        staged = append_fragment_atom_stage(staged, target, location.clone(), inline);
    }

    if let Some(children) = children {
        for (child_index, child) in children.into_iter().enumerate() {
            let mut child_path = location.child_path.clone();
            child_path.push(child_index);
            staged = append_node_stages(
                staged,
                target,
                FragmentLocation {
                    root_index: location.root_index,
                    child_path,
                },
                child,
            )?;
        }
    }

    Ok(staged)
}

/// Materializes detached clipboard atoms only after their leaf node exists.
/// The leaf starts atom-free, so same-anchor ordinals are reconstructed from
/// capture order and every atom receives a fresh canonical identity.
fn append_fragment_atom_stage(
    staged: StagedPlan,
    target: NodeId,
    location: FragmentLocation,
    inline: ClipboardInline,
) -> StagedPlan {
    staged.stage(move |document| {
        let leaf = resolve_location(document, target, &location)?;
        let target_inline = inline_of(document, leaf)?;
        if target_inline.len_bytes() != inline.len_bytes() {
            return Err(SessionError::SelectionInvalid);
        }

        let mut transaction = user_transaction();
        let mut previous_anchor = None;
        let mut ordinal = 0usize;
        for atom in inline.atoms() {
            let raw = atom.anchor().as_usize();
            if previous_anchor == Some(raw) {
                ordinal += 1;
            } else {
                previous_anchor = Some(raw);
                ordinal = 0;
            }
            let offset = target_inline.offset_at(raw).map_err(SessionError::Core)?;
            transaction.push_step(TransactionStep::InsertInlineAtom {
                at: InlinePoint::new(leaf, offset, ordinal, CursorAffinity::Before),
                kind: atom.kind().clone(),
                attrs: atom.attrs().clone(),
                content: atom.content().clone(),
            });
        }
        Ok(transaction)
    })
}

fn insertion_point(
    document: &XiaomuDocument,
    target: NodeId,
    location: &FragmentLocation,
) -> Result<(NodeId, usize), SessionError> {
    if location.child_path.is_empty() {
        let parent = document
            .parent_of(target)
            .ok_or(SessionError::SelectionInvalid)?;
        let target_index = children_of(document, parent)
            .iter()
            .position(|child| *child == target)
            .ok_or(SessionError::SelectionInvalid)?;
        return Ok((parent, target_index + 1 + location.root_index));
    }

    let child_index = *location
        .child_path
        .last()
        .ok_or(SessionError::SelectionInvalid)?;
    let mut parent_location = location.clone();
    parent_location.child_path.pop();
    let parent = resolve_location(document, target, &parent_location)?;
    Ok((parent, child_index))
}

fn resolve_location(
    document: &XiaomuDocument,
    target: NodeId,
    location: &FragmentLocation,
) -> Result<NodeId, SessionError> {
    let root_parent = document
        .parent_of(target)
        .ok_or(SessionError::SelectionInvalid)?;
    let root_children = children_of(document, root_parent);
    let target_index = root_children
        .iter()
        .position(|child| *child == target)
        .ok_or(SessionError::SelectionInvalid)?;
    let mut current = *root_children
        .get(target_index + 1 + location.root_index)
        .ok_or(SessionError::SelectionInvalid)?;

    for child_index in &location.child_path {
        current = *children_of(document, current)
            .get(*child_index)
            .ok_or(SessionError::SelectionInvalid)?;
    }
    Ok(current)
}

fn inline_of(document: &XiaomuDocument, node: NodeId) -> Result<&InlineContent, SessionError> {
    document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)
}
