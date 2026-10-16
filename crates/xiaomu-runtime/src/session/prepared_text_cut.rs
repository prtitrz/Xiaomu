//! Fail-closed source admission for the opt-in generic text Cut planner.

use xiaomu_core::document::{NodeContent, NodeId, NodeKind, XiaomuDocument};

use super::intent::PlannedAction;
use super::{DocumentPosition, DocumentSelection, EditPlan, PolicyError, SessionError};

#[cfg(test)]
thread_local! {
    static PLAN_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn plan_entries() -> usize {
    PLAN_ENTRIES.get()
}

/// Proof that projection and generic deletion cover the same inline leaves.
/// Fields are private: clipboard export cannot manufacture this admission.
pub(crate) struct ValidatedTextCut<'a> {
    document: &'a XiaomuDocument,
    selection: DocumentSelection,
}

impl<'a> ValidatedTextCut<'a> {
    pub(crate) fn source(&self) -> (&'a XiaomuDocument, DocumentSelection) {
        (self.document, self.selection)
    }
}

pub(super) fn plan<'a>(
    source: crate::clipboard::BudgetedTextCutSource<'a>,
    selection: DocumentSelection,
) -> Result<(ValidatedTextCut<'a>, EditPlan), SessionError> {
    #[cfg(test)]
    PLAN_ENTRIES.set(PLAN_ENTRIES.get() + 1);
    let document = source.document();
    let rejected = || {
        PolicyError::new("prepared text Cut requires a nonempty inline-only range outside tables")
    };
    if selection.is_collapsed()
        || selection.active_cell_range().is_some()
        || selection.as_node_selection().is_some()
    {
        return Err(rejected().into());
    }
    let (DocumentPosition::Inline(head), DocumentPosition::Inline(tail)) =
        selection.ordered(document)?
    else {
        return Err(rejected().into());
    };
    // The unforgeable source token bounds this full walk before recursion or
    // the existing planners can allocate owned payloads.
    // Include every non-inline leaf, empty container and entire table as a
    // barrier. Endpoint-only checks would miss images or a table in between.
    // Tables are opaque even for a single-cell range until that editing domain
    // has an independently verified merge/selection contract.
    let mut leaves = Vec::new();
    collect_leaves(document, document.root(), &mut leaves)?;
    let start = leaves.iter().position(|id| *id == head.node_id());
    let end = leaves.iter().position(|id| *id == tail.node_id());
    let (Some(start), Some(end)) = (start, end) else {
        return Err(rejected().into());
    };
    if start > end
        || leaves[start..=end].iter().any(|id| {
            !matches!(
                document.node(*id).map(|node| node.content()),
                Some(NodeContent::Inline(_))
            )
        })
    {
        return Err(rejected().into());
    }
    let action = if head.node_id() == tail.node_id() {
        let inline = document
            .node(head.node_id())
            .and_then(|node| node.content().as_inline())
            .ok_or(SessionError::SelectionInvalid)?;
        super::atom_edit::plan_delete(inline, Some(head), tail)?
    } else {
        super::cross_block_atom::plan_delete_selection(document, selection)?
    };
    let PlannedAction::Commit(plan) = action else {
        return Err(rejected().into());
    };
    Ok((
        ValidatedTextCut {
            document,
            selection,
        },
        plan,
    ))
}

fn collect_leaves(
    document: &XiaomuDocument,
    id: NodeId,
    leaves: &mut Vec<NodeId>,
) -> Result<(), SessionError> {
    let node = document.node(id).ok_or(SessionError::SelectionInvalid)?;
    if matches!(node.kind(), NodeKind::Table) {
        leaves.push(id);
    } else if let NodeContent::Children(children) = node.content()
        && !children.is_empty()
    {
        for child in children {
            collect_leaves(document, *child, leaves)?;
        }
    } else {
        leaves.push(id);
    }
    Ok(())
}
