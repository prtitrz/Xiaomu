//! Structural intent planning: split, join, and kind conversion.
//!
//! These planners emit Core structural steps plus the after-selection policy
//! the session will resolve against the resulting [`ChangeMap`]. They never
//! apply a transaction themselves.

use xiaomu_core::document::{NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::selection::TextSelection;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::SessionError;
use super::intent::{EditPlan, PlannedAction, SelectionUpdate};

/// Splits the focused inline block at the caret.
///
/// A non-collapsed selection is deleted first so Enter-over-selection is one
/// transaction and one history entry. After commit the caret belongs at the
/// start of the new tail sibling ([`SelectionUpdate::CaretAtSplitTail`]).
pub(crate) fn plan_split_block(selection: TextSelection) -> Result<PlannedAction, SessionError> {
    let node = selection.focus().node_id();
    let mut transaction = Transaction::new(TransactionOrigin::UserInput);

    let at = if selection.is_collapsed() {
        selection.focus().offset()
    } else {
        let range = selection
            .ordered_range()
            .map_err(|_| SessionError::SelectionInvalid)?;
        transaction.push_step(TransactionStep::ReplaceText {
            node,
            range,
            replacement: String::new(),
        });
        range.start()
    };

    transaction.push_step(TransactionStep::SplitNode { node, at });
    Ok(PlannedAction::Commit(EditPlan::new(
        transaction,
        SelectionUpdate::CaretAtSplitTail,
        None,
    )))
}

/// Joins `node` into its immediately preceding sibling.
///
/// No previous sibling is a legitimate no-op (Backspace at the start of the
/// first block). A preceding sibling that cannot be joined is left for Core
/// to reject atomically.
pub(crate) fn plan_join_with_previous(
    document: &XiaomuDocument,
    node: NodeId,
) -> Result<PlannedAction, SessionError> {
    let Some(first) = previous_join_target(document, node) else {
        return Ok(PlannedAction::NoChange);
    };

    Ok(PlannedAction::Commit(EditPlan::new(
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::JoinNodes {
            first,
            second: node,
        }),
        SelectionUpdate::CaretAtJoinSeam,
        None,
    )))
}

/// Changes `node`'s kind, keeping identity and content.
///
/// Requesting the kind the node already has is a no-op so history stays
/// clean. Incompatible shapes fail when Core applies the step.
pub(crate) fn plan_turn_into(
    document: &XiaomuDocument,
    node: NodeId,
    kind: &NodeKind,
) -> Result<PlannedAction, SessionError> {
    let current = document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?;
    if current.kind() == kind {
        return Ok(PlannedAction::NoChange);
    }

    Ok(PlannedAction::Commit(EditPlan::new(
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeKind {
            node,
            kind: kind.clone(),
        }),
        SelectionUpdate::MapExisting,
        None,
    )))
}

fn previous_join_target(document: &XiaomuDocument, node: NodeId) -> Option<NodeId> {
    let parent = document.parent_of(node)?;
    let children = document.node(parent)?.content().as_children()?;
    let index = children.iter().position(|child| *child == node)?;
    let first = index.checked_sub(1).map(|previous| children[previous])?;
    document.node(first)?.content().as_inline()?;
    document.node(node)?.content().as_inline()?;
    Some(first)
}
