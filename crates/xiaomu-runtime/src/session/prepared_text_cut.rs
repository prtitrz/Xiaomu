//! Fail-closed source admission for the opt-in generic text Cut planner.

use xiaomu_core::document::{NodeAttrs, NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::transaction::TransactionStep;

use super::intent::PlannedAction;
use super::{
    DocumentPosition, DocumentSelection, EditPlan, PolicyError, SelectionUpdate, SessionError,
};

#[cfg(test)]
thread_local! {
    static PLAN_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static ALL_PLAN_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn plan_entries() -> usize {
    PLAN_ENTRIES.get()
}

#[cfg(test)]
pub(super) fn all_plan_entries() -> usize {
    ALL_PLAN_ENTRIES.get()
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
    mut source: crate::clipboard::BudgetedTextCutSource<'a>,
    selection: DocumentSelection,
    all_paragraph_attrs: impl FnOnce() -> Result<Option<&'a NodeAttrs>, PolicyError>,
) -> Result<(ValidatedTextCut<'a>, EditPlan), SessionError> {
    #[cfg(test)]
    PLAN_ENTRIES.set(PLAN_ENTRIES.get() + 1);
    let document = source.document();
    let rejected = || {
        PolicyError::new(
            "prepared text Cut requires an inline-only range or exact text-only All outside tables",
        )
    };
    if selection.is_collapsed()
        || selection.active_cell_range().is_some()
        || selection.as_node_selection().is_some()
    {
        return Err(rejected().into());
    }
    if selection.is_all(document) {
        // All is structural, not a normalized full inline range. Check every
        // container and leaf before building a transaction, and keep the
        // original source for closed clipboard provenance and history.
        if !all_text_tree(document, document.root()) {
            return Err(rejected().into());
        }
        let default_attrs = NodeAttrs::empty();
        let attrs = all_paragraph_attrs()?.unwrap_or(&default_attrs);
        // The host lends its defaults. Account for them with the already
        // admitted source before any deep clone or candidate allocation.
        source.reserve_all_paragraph(attrs)?;
        #[cfg(test)]
        ALL_PLAN_ENTRIES.set(ALL_PLAN_ENTRIES.get() + 1);
        let children = document
            .node(document.root())
            .and_then(|node| node.content().as_children())
            .ok_or(SessionError::SelectionInvalid)?;
        let mut transaction =
            super::structure::user_transaction().with_step(TransactionStep::InsertNode {
                parent: document.root(),
                index: 0,
                kind: NodeKind::Paragraph,
                attrs: attrs.clone(),
                content: NodeContent::empty_inline(),
            });
        for child in children {
            transaction.push_step(TransactionStep::RemoveNode { node: *child });
        }
        return Ok((
            ValidatedTextCut {
                document,
                selection,
            },
            EditPlan::new(
                transaction,
                SelectionUpdate::CaretAtLastInsertedOffset { offset: 0 },
                None,
            ),
        ));
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

// Only the budgeted entry above calls this recursive, allocation-free scan.
// Empty inline blocks are real selected structure; empty containers are
// barriers. Unknown block semantics cannot opt in just by carrying text.
fn all_text_tree(document: &XiaomuDocument, id: NodeId) -> bool {
    let Some(node) = document.node(id) else {
        return false;
    };
    match (node.kind(), node.content()) {
        (
            NodeKind::Paragraph | NodeKind::Heading(_) | NodeKind::CodeBlock,
            NodeContent::Inline(_),
        ) => true,
        (
            NodeKind::Document
            | NodeKind::Quote
            | NodeKind::BulletList
            | NodeKind::OrderedList
            | NodeKind::ListItem
            | NodeKind::TaskList
            | NodeKind::TaskItem,
            NodeContent::Children(children),
        ) => !children.is_empty() && children.iter().all(|id| all_text_tree(document, *id)),
        _ => false,
    }
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
