//! SplitBlock planning: plain blocks split in place; list items create a
//! sibling item or exit when the focused block is empty.

use xiaomu_core::document::{NodeAttrs, NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::SessionError;
use super::atom_edit::{atoms_inside_span, text_selection_from};
use super::intent::{EditPlan, PlannedAction, SelectionUpdate};
use super::structure::{
    ListAncestry, StagedPlan, children_of, item_is_nested, list_ancestry_of, plan_lift_out_of_list,
    plan_outdent_list_item, subtree_payloads, user_transaction,
};

/// Splits the focused inline block at the caret.
///
/// Outside a list this splits at the full mixed-inline caret gap. Inside a
/// list item, a non-empty block becomes a new sibling item holding the tail;
/// an empty collapsed item leaves the current list level (outdent when nested,
/// lift out at the top). A non-collapsed selection is deleted first so the
/// whole gesture is one history entry.
pub(crate) fn plan_split_block(
    document: &XiaomuDocument,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    let node = focus.node_id();
    if let Some(ancestry) = list_ancestry_of(document, node) {
        return plan_list_enter(document, anchor, focus, ancestry);
    }
    plan_plain_split(document, anchor, focus)
}

fn plan_plain_split(
    document: &XiaomuDocument,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    Ok(PlannedAction::Commit(EditPlan::new(
        split_transaction(document, anchor, focus)?,
        SelectionUpdate::CaretAtSplitTail,
        None,
    )))
}

fn plan_list_enter(
    document: &XiaomuDocument,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
    ancestry: ListAncestry,
) -> Result<PlannedAction, SessionError> {
    let node = focus.node_id();
    if anchor.is_none_or(|anchor| anchor == focus) && is_empty_inline(document, node)? {
        if item_is_nested(document, &ancestry)? {
            return plan_outdent_list_item(document, node);
        }
        return plan_lift_out_of_list(document, ancestry);
    }
    plan_split_list_item(document, anchor, focus, ancestry)
}

fn plan_split_list_item(
    document: &XiaomuDocument,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
    ancestry: ListAncestry,
) -> Result<PlannedAction, SessionError> {
    let node = focus.node_id();
    let ListAncestry {
        item,
        list,
        item_index,
        ..
    } = ancestry;
    let split = split_transaction(document, anchor, focus)?;
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

/// Deletes a selected span before splitting; atom removals and the split
/// stay in one transaction so failures cannot publish partial edits.
fn split_transaction(
    document: &XiaomuDocument,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<Transaction, SessionError> {
    let node = focus.node_id();
    let inline = document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)?;
    let mut transaction = Transaction::new(TransactionOrigin::UserInput);
    if inline.atoms().is_empty() {
        // Preserve the legacy text-only steps and maps exactly.
        let selection = text_selection_from(anchor, focus)?;
        let at = if selection.is_collapsed() {
            focus.text_offset()
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
        return Ok(transaction);
    }

    let at = if let Some(anchor) = anchor.filter(|anchor| *anchor != focus) {
        let key = |point: InlinePoint| (point.text_offset(), point.atom_index());
        let (start, end) = if key(anchor) <= key(focus) {
            (anchor, focus)
        } else {
            (focus, anchor)
        };
        for atom in atoms_inside_span(inline, start, end) {
            transaction.push_step(TransactionStep::RemoveInlineAtom { atom });
        }
        // The surviving start-seam atoms remain before the original ordinal.
        // End-seam atoms shift to that boundary after text removal.
        transaction.push_step(TransactionStep::ReplaceInlineText {
            at: start,
            end: end.text_offset(),
            replacement: String::new(),
        });
        start
    } else {
        focus
    };
    transaction.push_step(TransactionStep::SplitInlineNode { at });
    Ok(transaction)
}

fn is_empty_inline(document: &XiaomuDocument, node: NodeId) -> Result<bool, SessionError> {
    let inline = document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)?;
    Ok(inline.len_bytes() == 0 && inline.atoms().is_empty())
}
