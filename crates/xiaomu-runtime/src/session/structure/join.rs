//! Cross-container joins preserve complete mixed-inline subtrees.

use xiaomu_core::document::{NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::TransactionStep;

use super::{children_of, last_inline_descendant, subtree_payloads, user_transaction};
use crate::session::SessionError;
use crate::session::intent::{EditPlan, PrimaryEdit, SelectionUpdate, concatenated};

/// Joins `node`'s text into the last inline block of `container`.
///
/// Shared by two backspace shapes: a plain block whose preceding sibling is
/// a list appends into the list's tail, and a list item with a previous
/// sibling item appends into that item's tail (the editor-standard "delete
/// the bullet by merging upward"). The emptied source block is removed; an
/// emptied parent item dissolves with it. The caret lands at the join seam
/// via [`SelectionUpdate::CaretAfterReplacement`].
pub(super) fn plan_join_block_into_container_tail(
    document: &XiaomuDocument,
    node: NodeId,
    container: NodeId,
) -> Result<Option<EditPlan>, SessionError> {
    let Some(target) = last_inline_descendant(document, container) else {
        return Ok(None);
    };

    let focus_inline = document
        .node(node)
        .ok_or(SessionError::SelectionInvalid)?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)?;
    let tail_inline = document
        .node(target)
        .ok_or(SessionError::SelectionInvalid)?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)?;
    if !focus_inline.atoms().is_empty() || !tail_inline.atoms().is_empty() {
        return mixed_join(document, node, target).map(Some);
    }
    let moved_text = concatenated(focus_inline);
    let seam = tail_inline
        .offset_at(concatenated(tail_inline).len())
        .map_err(SessionError::Core)?;

    let mut transaction = user_transaction().with_step(TransactionStep::ReplaceText {
        node: target,
        range: TextRange::new(seam, seam).map_err(SessionError::Core)?,
        replacement: moved_text.clone(),
    });

    // The focused block goes first; removing it may leave its own list
    // item empty, and that item dissolves right after.
    transaction.push_step(TransactionStep::RemoveNode { node });
    let parent = document
        .parent_of(node)
        .ok_or(SessionError::SelectionInvalid)?;
    if document
        .node(parent)
        .ok_or(SessionError::SelectionInvalid)?
        .kind()
        == &NodeKind::ListItem
    {
        let siblings = children_of(document, parent);
        if siblings.len() == 1 && siblings[0] == node {
            transaction.push_step(TransactionStep::RemoveNode { node: parent });
        }
    }

    Ok(Some(EditPlan::new(
        transaction,
        SelectionUpdate::CaretAtJoinPoint,
        Some(PrimaryEdit {
            node: target,
            range: TextRange::new(seam, seam).map_err(SessionError::Core)?,
            inserted_len: moved_text.len(),
        }),
    )))
}

/// Bring the whole source beside the target, then let Core join the two
/// blocks. This preserves identities, text marks, opaque atom payloads and
/// the exact seam ordinal without serializing through a text fallback.
fn mixed_join(
    document: &XiaomuDocument,
    node: NodeId,
    target: NodeId,
) -> Result<EditPlan, SessionError> {
    let source_parent = document
        .parent_of(node)
        .ok_or(SessionError::SelectionInvalid)?;
    let target_parent = document
        .parent_of(target)
        .ok_or(SessionError::SelectionInvalid)?;
    let target_index = children_of(document, target_parent)
        .iter()
        .position(|child| *child == target)
        .ok_or(SessionError::SelectionInvalid)?;
    let mut transaction = user_transaction();
    transaction.push_step(TransactionStep::RemoveNode { node });
    transaction.push_step(TransactionStep::RestoreSubtree {
        parent: target_parent,
        index: target_index + 1,
        root: node,
        nodes: subtree_payloads(document, node),
    });
    transaction.push_step(TransactionStep::JoinNodes {
        first: target,
        second: node,
    });
    if document
        .node(source_parent)
        .ok_or(SessionError::SelectionInvalid)?
        .kind()
        == &NodeKind::ListItem
        && children_of(document, source_parent) == [node]
    {
        transaction.push_step(TransactionStep::RemoveNode {
            node: source_parent,
        });
    }
    Ok(EditPlan::new(
        transaction,
        SelectionUpdate::CaretAtJoinSeam,
        None,
    ))
}
