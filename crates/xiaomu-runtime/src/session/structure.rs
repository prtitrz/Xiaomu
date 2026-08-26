//! Structural intent planning: split, join, kind conversion, and list
//! commands.
//!
//! These planners emit Core structural steps plus the after-selection policy
//! the session will resolve against the resulting [`ChangeMap`]. They never
//! apply a transaction themselves.

use xiaomu_core::document::{Node, NodeAttrs, NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::selection::TextSelection;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::SessionError;
use super::intent::{EditPlan, PlannedAction, SelectionUpdate};

/// A transaction built lazily from the snapshot its stage sees.
pub(crate) type StagedTransaction =
    Box<dyn FnOnce(&XiaomuDocument) -> Result<Transaction, SessionError>>;

/// A command applied as several transactions that share one history entry.
///
/// List wrap and indent must reference containers that Core allocates only
/// during application. Each stage is built from the snapshot the previous
/// stages produced, so it addresses freshly created nodes by their
/// deterministic positions instead of by unknown identities. Application is
/// all-or-nothing: any failing stage aborts the whole command with the
/// session state untouched.
pub(crate) struct StagedPlan {
    pub(crate) stages: Vec<StagedTransaction>,
    pub(crate) selection_update: SelectionUpdate,
}

impl StagedPlan {
    pub(crate) fn new(selection_update: SelectionUpdate) -> Self {
        Self {
            stages: Vec::new(),
            selection_update,
        }
    }

    pub(crate) fn stage(
        mut self,
        build: impl FnOnce(&XiaomuDocument) -> Result<Transaction, SessionError> + 'static,
    ) -> Self {
        self.stages.push(Box::new(build));
        self
    }
}

fn user_transaction() -> Transaction {
    Transaction::new(TransactionOrigin::UserInput)
}

fn kind_of(document: &XiaomuDocument, node: NodeId) -> Result<&NodeKind, SessionError> {
    Ok(document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .kind())
}

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
///
/// List kinds compose with the surrounding structure instead of rewriting
/// one node's kind: an inline block becomes a single-item list (wrap), an
/// item's block returns to a plain sibling block (lift out), and a list
/// converts between bullet and ordered by rekinding the list itself.
pub(crate) fn plan_turn_into(
    document: &XiaomuDocument,
    node: NodeId,
    kind: &NodeKind,
) -> Result<PlannedAction, SessionError> {
    if matches!(kind, NodeKind::BulletList | NodeKind::OrderedList) {
        return plan_into_list(document, node, kind);
    }

    if matches!(kind, NodeKind::Paragraph)
        && let Some(ancestry) = list_ancestry_of(document, node)
    {
        return plan_lift_out_of_list(document, ancestry);
    }

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

/// Indents the focused block's list item under its previous sibling item.
///
/// The first item cannot indent. When the previous item already ends with a
/// nested list the item moves into it; otherwise a nested list of the same
/// kind is created first (a staged command). The focused block keeps its
/// identity, so the caret stays at its offset.
pub(crate) fn plan_indent_list_item(
    document: &XiaomuDocument,
    node: NodeId,
) -> Result<PlannedAction, SessionError> {
    let Some(ancestry) = list_ancestry_of(document, node) else {
        return Ok(PlannedAction::NoChange);
    };
    if ancestry.item_index == 0 {
        return Ok(PlannedAction::NoChange);
    }

    let siblings = children_of(document, ancestry.list);
    let previous_item = siblings[ancestry.item_index - 1];
    let payloads = subtree_payloads(document, ancestry.item);

    if let Some(inner_list) = trailing_list_of(document, previous_item) {
        let inner_len = children_of(document, inner_list).len();
        return Ok(PlannedAction::Commit(EditPlan::new(
            move_item_transaction(ancestry.item, inner_list, inner_len, payloads),
            SelectionUpdate::PreserveFocus,
            None,
        )));
    }

    // Create the nested list first; the second stage addresses it by its
    // deterministic position as the previous item's last child.
    let list_kind = kind_of(document, ancestry.list)?.clone();
    let position = children_of(document, previous_item).len();
    let staged = StagedPlan::new(SelectionUpdate::PreserveFocus)
        .stage(move |_| {
            Ok(user_transaction().with_step(TransactionStep::InsertNode {
                parent: previous_item,
                index: position,
                kind: list_kind,
                attrs: NodeAttrs::empty(),
                content: NodeContent::children([]),
            }))
        })
        .stage(move |document| {
            let created = children_of(document, previous_item);
            let inner_list = *created.last().ok_or(SessionError::SelectionInvalid)?;
            let payloads = subtree_payloads(document, ancestry.item);
            Ok(move_item_transaction(
                ancestry.item,
                inner_list,
                0,
                payloads,
            ))
        });
    Ok(PlannedAction::CommitStaged(staged))
}

/// Outdents the focused block's nested list item into the outer list.
///
/// The model forbids an item directly inside an item, so the item re-enters
/// the list that contains its current list, directly after the outer item.
/// A top-level list item cannot outdent. The emptied inner list dissolves.
pub(crate) fn plan_outdent_list_item(
    document: &XiaomuDocument,
    node: NodeId,
) -> Result<PlannedAction, SessionError> {
    let Some(ancestry) = list_ancestry_of(document, node) else {
        return Ok(PlannedAction::NoChange);
    };
    if kind_of(document, ancestry.list_parent)? != &NodeKind::ListItem {
        return Ok(PlannedAction::NoChange);
    }
    let outer_list = document
        .parent_of(ancestry.list_parent)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?;
    if !matches!(
        kind_of(document, outer_list)?,
        NodeKind::BulletList | NodeKind::OrderedList
    ) {
        return Ok(PlannedAction::NoChange);
    }

    let outer_index = children_of(document, outer_list)
        .iter()
        .position(|child| *child == ancestry.list_parent)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?;
    let payloads = subtree_payloads(document, ancestry.item);
    let dissolves = children_of(document, ancestry.list).len() == 1;

    let mut transaction =
        move_item_transaction(ancestry.item, outer_list, outer_index + 1, payloads);
    if dissolves {
        transaction.push_step(TransactionStep::RemoveNode {
            node: ancestry.list,
        });
    }

    Ok(PlannedAction::Commit(EditPlan::new(
        transaction,
        SelectionUpdate::PreserveFocus,
        None,
    )))
}

fn plan_into_list(
    document: &XiaomuDocument,
    node: NodeId,
    kind: &NodeKind,
) -> Result<PlannedAction, SessionError> {
    if let Some(ancestry) = list_ancestry_of(document, node) {
        if kind_of(document, ancestry.list)? == kind {
            return Ok(PlannedAction::NoChange);
        }
        // Bullet ↔ ordered keeps every identity; positions do not move.
        return Ok(PlannedAction::Commit(EditPlan::new(
            user_transaction().with_step(TransactionStep::SetNodeKind {
                node: ancestry.list,
                kind: kind.clone(),
            }),
            SelectionUpdate::MapExisting,
            None,
        )));
    }

    document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)?;
    let parent = document
        .parent_of(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?;
    let index = children_of(document, parent)
        .iter()
        .position(|child| *child == node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?;

    // Stage 1 creates the empty list right after the block, stage 2 the
    // empty item inside it, and stage 3 moves the block into the item —
    // each stage addresses the freshly created containers by their
    // deterministic positions in the snapshot it sees.
    let target = kind.clone();
    let staged = StagedPlan::new(SelectionUpdate::PreserveFocus)
        .stage(move |_| {
            Ok(user_transaction().with_step(TransactionStep::InsertNode {
                parent,
                index: index + 1,
                kind: target,
                attrs: NodeAttrs::empty(),
                content: NodeContent::children([]),
            }))
        })
        .stage(move |document| {
            let list = children_of(document, parent)[index + 1];
            Ok(user_transaction().with_step(TransactionStep::InsertNode {
                parent: list,
                index: 0,
                kind: NodeKind::ListItem,
                attrs: NodeAttrs::empty(),
                content: NodeContent::children([]),
            }))
        })
        .stage(move |document| {
            let list = children_of(document, parent)[index + 1];
            let item = children_of(document, list)[0];
            let mut transaction = user_transaction();
            transaction.push_step(TransactionStep::RemoveNode { node });
            transaction.push_step(TransactionStep::RestoreSubtree {
                parent: item,
                index: 0,
                root: node,
                nodes: subtree_payloads(document, node),
            });
            Ok(transaction)
        });
    Ok(PlannedAction::CommitStaged(staged))
}

/// Lifts every child of a list item back to the list's own level.
///
/// Single-item lists dissolve entirely, which closes the paragraph → list →
/// paragraph loop. Multi-item lists keep their remaining items; only the
/// focused item dissolves into plain sibling blocks.
fn plan_lift_out_of_list(
    document: &XiaomuDocument,
    ancestry: ListAncestry,
) -> Result<PlannedAction, SessionError> {
    let ListAncestry {
        item,
        list,
        list_parent,
        list_index,
        ..
    } = ancestry;
    let moved = children_of(document, item);
    if moved.is_empty() {
        return Ok(PlannedAction::NoChange);
    }
    let dissolves = children_of(document, list).len() == 1;

    let mut transaction = user_transaction();
    for child in &moved {
        transaction.push_step(TransactionStep::RemoveNode { node: *child });
    }
    for (offset, child) in moved.iter().enumerate() {
        transaction.push_step(TransactionStep::RestoreSubtree {
            parent: list_parent,
            // Lifted blocks take the list's own slot, appearing above the
            // remaining list content.
            index: list_index + offset,
            root: *child,
            nodes: subtree_payloads(document, *child),
        });
    }
    transaction.push_step(TransactionStep::RemoveNode { node: item });
    if dissolves {
        transaction.push_step(TransactionStep::RemoveNode { node: list });
    }

    Ok(PlannedAction::Commit(EditPlan::new(
        transaction,
        SelectionUpdate::PreserveFocus,
        None,
    )))
}

/// A transaction that moves one subtree below a new parent at `index`.
fn move_item_transaction(
    item: NodeId,
    parent: NodeId,
    index: usize,
    payloads: Vec<Node>,
) -> Transaction {
    let mut transaction = user_transaction();
    transaction.push_step(TransactionStep::RemoveNode { node: item });
    transaction.push_step(TransactionStep::RestoreSubtree {
        parent,
        index,
        root: item,
        nodes: payloads,
    });
    transaction
}

/// Enclosing list structure of an inline block, all resolved eagerly so
/// planners can build transactions from stable coordinates.
pub(crate) struct ListAncestry {
    /// The list item containing the focused block.
    item: NodeId,
    /// The bullet/ordered list containing `item`.
    list: NodeId,
    /// The parent of `list` (root, quote, or another list item).
    list_parent: NodeId,
    /// Index of `list` among the parent's children.
    list_index: usize,
    /// Index of `item` among the list's children.
    item_index: usize,
}

fn list_ancestry_of(document: &XiaomuDocument, block: NodeId) -> Option<ListAncestry> {
    let item = document.parent_of(block)?;
    if document.node(item)?.kind() != &NodeKind::ListItem {
        return None;
    }
    let list = document.parent_of(item)?;
    if !matches!(
        document.node(list)?.kind(),
        NodeKind::BulletList | NodeKind::OrderedList
    ) {
        return None;
    }
    let list_parent = document.parent_of(list)?;
    let list_index = children_of(document, list_parent)
        .iter()
        .position(|child| *child == list)?;
    let item_index = children_of(document, list)
        .iter()
        .position(|child| *child == item)?;
    Some(ListAncestry {
        item,
        list,
        list_parent,
        list_index,
        item_index,
    })
}

/// Returns the last child of `node` when it is a list container.
fn trailing_list_of(document: &XiaomuDocument, node: NodeId) -> Option<NodeId> {
    let last = *children_of(document, node).last()?;
    matches!(
        document.node(last)?.kind(),
        NodeKind::BulletList | NodeKind::OrderedList
    )
    .then_some(last)
}

fn children_of(document: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    document
        .node(node)
        .and_then(|node| node.content().as_children())
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default()
}

/// Snapshots of every node in `root`'s subtree, for exact restoration via
/// [`TransactionStep::RestoreSubtree`].
fn subtree_payloads(document: &XiaomuDocument, root: NodeId) -> Vec<Node> {
    let mut payloads = Vec::new();
    let mut queue = std::collections::VecDeque::from([root]);
    while let Some(current) = queue.pop_front() {
        let Some(node) = document.node(current) else {
            continue;
        };
        if let NodeContent::Children(children) = node.content() {
            queue.extend(children.iter().copied());
        }
        payloads.push(node.clone());
    }
    payloads
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
