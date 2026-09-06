//! Image block insertion (P4.7).
//!
//! The InsertImage command inserts a `NodeKind::Image` atomic block as a
//! sibling right after the focused block. The typed payload serializes into
//! canonical attrs; pixels and host file objects stay outside the document.

use xiaomu_core::document::{ImageAttrs, NodeContent, NodeId, NodeKind};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::intent::HistoryPolicy;
use super::intent::{EditPlan, PlannedAction, SelectionUpdate};
use super::selection::DocumentPosition;
use super::{DocumentSession, SessionError};

impl DocumentSession {
    /// Plans the InsertImage command: the image block becomes the next
    /// sibling of the focused block and the caret stays where it was.
    pub(crate) fn plan_insert_image(
        &self,
        image: &ImageAttrs,
    ) -> Result<PlannedAction, SessionError> {
        let (head, _) = self.selection.ordered(&self.document)?;
        let DocumentPosition::Inline(head) = head else {
            return Err(SessionError::SelectionInvalid);
        };
        let parent = self
            .document
            .parent_of(head.node_id())
            .ok_or(SessionError::SelectionInvalid)?;
        let position = self
            .document
            .node(parent)
            .and_then(|parent| parent.content().as_children().map(<[NodeId]>::to_vec))
            .ok_or(SessionError::SelectionInvalid)?
            .iter()
            .position(|child| *child == head.node_id())
            .ok_or(SessionError::SelectionInvalid)?;
        let mut transaction = Transaction::new(TransactionOrigin::UserInput);
        transaction.push_step(TransactionStep::InsertNode {
            parent,
            index: position + 1,
            kind: NodeKind::Image,
            attrs: image.to_attrs().map_err(SessionError::Core)?,
            content: NodeContent::Atomic,
        });
        Ok(PlannedAction::Commit(
            EditPlan::new(transaction, SelectionUpdate::MapExisting, None)
                .with_history_policy(HistoryPolicy::Isolated),
        ))
    }
}
