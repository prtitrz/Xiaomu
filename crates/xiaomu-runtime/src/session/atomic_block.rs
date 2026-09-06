//! Atomic block selection and removal (P4.6).
//!
//! Atomic-content blocks (`HorizontalRule`, `Image`, ...) carry no editable
//! interior. The session addresses them as whole-node selections: the caret
//! selects the block, Backspace / Delete remove it as one logical history
//! change, and the selection converges to the structural gap the block
//! occupied. IME and text intents never enter an atomic interior.

use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_core::selection::NodeGap;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::intent::{EditPlan, PlannedAction, SelectionUpdate};
use super::selection::DocumentSelection;
use super::{DocumentSession, SessionError, SessionOutcome};

impl DocumentSession {
    /// Selects one atomic-content block as a whole node.
    ///
    /// The node must exist and carry atomic content; text-bearing and
    /// container nodes cannot be node-selected in this contract.
    pub fn set_atomic_selection(&mut self, node: NodeId) -> Result<SessionOutcome, SessionError> {
        require_atomic(&self.document, node)?;
        self.install_selection(DocumentSelection::collapsed(node))
    }

    /// Removes the collapsed atomic node selection as one history entry.
    ///
    /// The selection converges to the gap the removed block occupied, which
    /// is always a valid structural boundary in the post-command snapshot.
    pub(super) fn plan_atomic_removal(&self) -> Result<PlannedAction, SessionError> {
        let Some(node) = self.selection.as_atomic_node() else {
            return Ok(PlannedAction::NoChange);
        };
        require_atomic(&self.document, node)?;
        let parent = self
            .document
            .parent_of(node)
            .ok_or(SessionError::SelectionInvalid)?;
        let children = self
            .document
            .node(parent)
            .and_then(|parent| parent.content().as_children().map(<[NodeId]>::to_vec))
            .ok_or(SessionError::SelectionInvalid)?;
        let index = children
            .iter()
            .position(|child| *child == node)
            .ok_or(SessionError::SelectionInvalid)?;

        let transaction = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::RemoveNode { node });
        Ok(PlannedAction::Commit(EditPlan::new(
            transaction,
            SelectionUpdate::CaretAtGap {
                gap: NodeGap::new(parent, index),
            },
            None,
        )))
    }
}

fn require_atomic(document: &XiaomuDocument, node: NodeId) -> Result<(), SessionError> {
    match document.node(node) {
        Some(node) if node.content().is_atomic() => Ok(()),
        _ => Err(SessionError::SelectionInvalid),
    }
}
