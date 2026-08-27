//! SplitBlock planning: plain blocks split in place; list items create a
//! sibling item or exit when the focused block is empty.

use xiaomu_core::document::{NodeAttrs, NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::selection::TextSelection;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::SessionError;
use super::intent::{EditPlan, PlannedAction, SelectionUpdate};
use super::structure::{
    ListAncestry, StagedPlan, children_of, item_is_nested, list_ancestry_of, plan_lift_out_of_list,
    plan_outdent_list_item, subtree_payloads, user_transaction,
};

/// Splits the focused inline block at the caret.
///
/// Outside a list this is a plain [`TransactionStep::SplitNode`]. Inside a
/// list item, a non-empty block becomes a new sibling item holding the tail;
/// an empty collapsed item leaves the current list level (outdent when nested,
/// lift out at the top). A non-collapsed selection is deleted first so the
/// whole gesture is one history entry.
pub(crate) fn plan_split_block(
    document: &XiaomuDocument,
    selection: TextSelection,
) -> Result<PlannedAction, SessionError> {
    let node = selection.focus().node_id();
    if let Some(ancestry) = list_ancestry_of(document, node) {
        return plan_list_enter(document, selection, ancestry);
    }
    plan_plain_split(selection)
}

fn plan_plain_split(selection: TextSelection) -> Result<PlannedAction, SessionError> {
    Ok(PlannedAction::Commit(EditPlan::new(
        split_transaction(selection)?,
        SelectionUpdate::CaretAtSplitTail,
        None,
    )))
}

fn plan_list_enter(
    document: &XiaomuDocument,
    selection: TextSelection,
    ancestry: ListAncestry,
) -> Result<PlannedAction, SessionError> {
    let node = selection.focus().node_id();
    if selection.is_collapsed() && is_empty_inline(document, node)? {
        if item_is_nested(document, &ancestry)? {
            return plan_outdent_list_item(document, node);
        }
        return plan_lift_out_of_list(document, ancestry);
    }
    plan_split_list_item(selection, ancestry)
}

fn plan_split_list_item(
    selection: TextSelection,
    ancestry: ListAncestry,
) -> Result<PlannedAction, SessionError> {
    let node = selection.focus().node_id();
    let ListAncestry {
        item,
        list,
        item_index,
        ..
    } = ancestry;
    let split = split_transaction(selection)?;
    let staged = StagedPlan::new(SelectionUpdate::CaretAtSplitTail)
        .stage(move |_| Ok(split))
        .stage(move |_| {
            Ok(user_transaction().with_step(TransactionStep::InsertNode {
                parent: list,
                index: item_index + 1,
                kind: NodeKind::ListItem,
                attrs: NodeAttrs::empty(),
                content: NodeContent::children([]),
            }))
        })
        .stage(move |document| {
            let siblings = children_of(document, item);
            let position = siblings
                .iter()
                .position(|child| *child == node)
                .ok_or(SessionError::SelectionInvalid)?;
            let tail = *siblings
                .get(position + 1)
                .ok_or(SessionError::SelectionInvalid)?;
            let new_item = *children_of(document, list)
                .get(item_index + 1)
                .ok_or(SessionError::SelectionInvalid)?;
            let mut transaction = user_transaction();
            transaction.push_step(TransactionStep::RemoveNode { node: tail });
            transaction.push_step(TransactionStep::RestoreSubtree {
                parent: new_item,
                index: 0,
                root: tail,
                nodes: subtree_payloads(document, tail),
            });
            Ok(transaction)
        });
    Ok(PlannedAction::CommitStaged(staged))
}

fn split_transaction(selection: TextSelection) -> Result<Transaction, SessionError> {
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
    Ok(transaction)
}

fn is_empty_inline(document: &XiaomuDocument, node: NodeId) -> Result<bool, SessionError> {
    Ok(document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)?
        .len_bytes()
        == 0)
}
