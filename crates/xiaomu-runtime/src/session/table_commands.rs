//! Selection-driven, host-neutral merge/split command entry points.
//!
//! Core owns cell allocation and exact inverses. Product-specific blank-block
//! filtering, cell factories and rectangle rebuilding belong in host policy.

use xiaomu_core::document::{NodeId, NodeKind};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::intent::PlannedAction;
use super::{
    DocumentPosition, DocumentSession, EditIntent, EditPlan, SelectionUpdate, SessionError,
    SessionOutcome,
};

impl DocumentSession {
    /// Runs before default structural selection guards, after host preflight.
    pub(super) fn apply_table_cell_intent(
        &mut self,
        intent: &EditIntent,
    ) -> Result<Option<SessionOutcome>, SessionError> {
        let action = match intent {
            EditIntent::MergeTableCells => self.plan_merge_table_cells()?,
            EditIntent::SplitTableCell => self.plan_split_table_cell()?,
            _ => return Ok(None),
        };
        match action {
            PlannedAction::NoChange => Ok(Some(SessionOutcome::NoChange)),
            PlannedAction::Commit(plan) => {
                // Do not change marks or grouping until eligibility and plan
                // construction succeed. The public intent wrapper restores
                // both if Core, selection or host candidate validation fails.
                self.history.break_group();
                self.clear_stored_marks();
                self.commit(plan).map(Some)
            }
            PlannedAction::CommitStaged(_) => unreachable!("single Core table-cell step"),
        }
    }

    fn plan_merge_table_cells(&self) -> Result<PlannedAction, SessionError> {
        let Some(range) = self.selection.active_cell_range() else {
            return Ok(PlannedAction::NoChange);
        };
        if !range.is_closed_rect(&self.document)? || range.unique_origins(&self.document)?.len() < 2
        {
            return Ok(PlannedAction::NoChange);
        }
        let table = self.table_of_command_cell(range.anchor())?;
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::MergeTableCells {
                table,
                rect: range.logical_rect(&self.document)?,
            },
        );
        Ok(PlannedAction::Commit(EditPlan::new(
            transaction,
            SelectionUpdate::MapExisting,
            None,
        )))
    }

    fn plan_split_table_cell(&self) -> Result<PlannedAction, SessionError> {
        let cell = if let Some(range) = self.selection.active_cell_range() {
            if range.anchor() != range.focus() {
                return Ok(PlannedAction::NoChange);
            }
            range.anchor()
        } else {
            if !self.selection.is_collapsed() {
                return Ok(PlannedAction::NoChange);
            }
            let Some(cell) = self.command_caret_cell() else {
                return Ok(PlannedAction::NoChange);
            };
            cell
        };
        let table = self.table_of_command_cell(cell)?;
        let grid = self.document.table_grid(table)?;
        let placement = grid.placement(cell).ok_or(SessionError::SelectionInvalid)?;
        if placement.rowspan() == 1 && placement.colspan() == 1 {
            return Ok(PlannedAction::NoChange);
        }
        let transaction = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::SplitTableCell { table, cell });
        Ok(PlannedAction::Commit(EditPlan::new(
            transaction,
            // A single-cell range retains the survivor identity and exact
            // parked position. Its footprint becomes one unit cell; Runtime
            // never predicts fresh identities to recreate the old rectangle.
            SelectionUpdate::PreserveSelection,
            None,
        )))
    }

    fn table_of_command_cell(&self, cell: NodeId) -> Result<NodeId, SessionError> {
        let row = self
            .document
            .parent_of(cell)
            .ok_or(SessionError::SelectionInvalid)?;
        let table = self
            .document
            .parent_of(row)
            .ok_or(SessionError::SelectionInvalid)?;
        if !self
            .document
            .node(row)
            .is_some_and(|node| matches!(node.kind(), NodeKind::TableRow))
            || !self
                .document
                .node(table)
                .is_some_and(|node| matches!(node.kind(), NodeKind::Table))
        {
            return Err(SessionError::SelectionInvalid);
        }
        Ok(table)
    }

    /// The innermost cell containing a collapsed inline, atomic or gap caret.
    fn command_caret_cell(&self) -> Option<NodeId> {
        let mut current = match self.selection.focus() {
            DocumentPosition::Inline(point) => point.node_id(),
            DocumentPosition::Atomic(node) => node,
            DocumentPosition::Gap(gap) => gap.parent(),
        };
        loop {
            if self.document.node(current)?.kind().is_table_cell() {
                return Some(current);
            }
            current = self.document.parent_of(current)?;
        }
    }
}
