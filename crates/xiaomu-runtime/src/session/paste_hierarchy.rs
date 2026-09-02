//! Hierarchical structured clipboard paste planning.
//!
//! Container fragments cannot be reconstructed in one Core transaction today:
//! `InsertNode` allocates a fresh identity, while later child inserts need that
//! identity as their parent. Runtime therefore uses the existing staged-command
//! seam. Intermediate snapshots never escape and the whole paste still records
//! exactly one history entry.

use xiaomu_core::document::{InlineContent, NodeContent, NodeId, XiaomuDocument};
use xiaomu_core::transaction::{Transaction, TransactionStep};

use crate::clipboard::{ClipboardNode, ClipboardNodeContent, ClipboardSlice};

use super::cross_block;
use super::intent::{PlannedAction, SelectionUpdate};
use super::structure::{StagedPlan, children_of, user_transaction};
use super::{DocumentPosition, DocumentSelection, SessionError};

#[derive(Clone)]
struct FragmentLocation {
    root_index: usize,
    child_path: Vec<usize>,
}

/// Reconstructs a clipboard fragment tree between the target block's prefix
/// and suffix. The target selection is deleted first, then the surviving block
/// is split at the deletion seam. Fragment roots are inserted before that
/// suffix and container children are filled in subsequent hidden stages.
pub(crate) fn plan_paste_hierarchy(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    slice: &ClipboardSlice,
) -> Result<PlannedAction, SessionError> {
    if slice
        .blocks()
        .iter()
        .any(|block| !block.inline().atoms().is_empty())
    {
        // Hierarchical paste inserts nodes through staged transactions that
        // cannot address a fresh block's identity inside the same stage; a
        // fragment with atoms would have to be downgraded to its plain-text
        // fallback, so it fails closed instead.
        return Err(SessionError::ClipboardAtomsUnsupported);
    }
    let last_offset = slice
        .blocks()
        .last()
        .map(|block| block.inline().len_bytes())
        .ok_or(SessionError::SelectionInvalid)?;
    let (target, initial) = split_target_transaction(document, selection)?;

    let mut staged = StagedPlan::new(SelectionUpdate::CaretAtLastInsertedOffset {
        offset: last_offset,
    })
    .stage(move |_| Ok(initial));

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

fn split_target_transaction(
    document: &XiaomuDocument,
    selection: DocumentSelection,
) -> Result<(NodeId, Transaction), SessionError> {
    selection
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;

    if let Some(single) = selection.as_single_node() {
        let target = single.focus().node_id();
        let range = single
            .ordered_range()
            .map_err(|_| SessionError::SelectionInvalid)?;
        let mut transaction = user_transaction();
        if !range.is_empty() {
            transaction.push_step(TransactionStep::ReplaceText {
                node: target,
                range,
                replacement: String::new(),
            });
        }
        transaction.push_step(TransactionStep::SplitNode {
            node: target,
            at: range.start(),
        });
        return Ok((target, transaction));
    }

    let (head, _) = selection.ordered(document)?;
    let DocumentPosition::Inline(head) = head else {
        return Err(SessionError::SelectionInvalid);
    };
    let action = cross_block::plan_delete_selection(document, selection)?;
    let PlannedAction::Commit(delete_plan) = action else {
        return Err(SessionError::SelectionInvalid);
    };
    let mut transaction = delete_plan.transaction().clone();
    transaction.push_step(TransactionStep::SplitNode {
        node: head.node_id(),
        at: head.text_offset(),
    });
    Ok((head.node_id(), transaction))
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
    let content = match node.content() {
        ClipboardNodeContent::Inline(inline) => NodeContent::Inline(
            InlineContent::new(inline.runs().iter().cloned())
                .map_err(SessionError::Core)?,
        ),
        ClipboardNodeContent::Children(_) => NodeContent::children([]),
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

    if let ClipboardNodeContent::Children(children) = node.content() {
        for (child_index, child) in children.iter().cloned().enumerate() {
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
