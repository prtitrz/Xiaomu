//! Rectangular selection operations. Clearing preserves table/cell identity
//! and attributes; content replacement is one isolated, reversible command.

use super::intent::{EditPlan, HistoryPolicy, PlannedAction, SelectionUpdate};
use super::structure::{children_of, user_transaction};
use super::{CellRange, DocumentSession, EditIntent, SessionError, SessionOutcome};
use xiaomu_core::document::{NodeAttrs, NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::transaction::TransactionStep;

pub(super) fn range_cells(
    document: &XiaomuDocument,
    range: CellRange,
) -> Result<Vec<Vec<NodeId>>, SessionError> {
    range.validate(document)?;
    let locate = |cell| {
        let row = document
            .parent_of(cell)
            .ok_or(SessionError::SelectionInvalid)?;
        let table = document
            .parent_of(row)
            .ok_or(SessionError::SelectionInvalid)?;
        let r = children_of(document, table)
            .iter()
            .position(|id| *id == row)
            .ok_or(SessionError::SelectionInvalid)?;
        let c = children_of(document, row)
            .iter()
            .position(|id| *id == cell)
            .ok_or(SessionError::SelectionInvalid)?;
        Ok::<_, SessionError>((table, r, c))
    };
    let (table, ar, ac) = locate(range.anchor())?;
    let (_, fr, fc) = locate(range.focus())?;
    let rows = children_of(document, table);
    Ok(rows[ar.min(fr)..=ar.max(fr)]
        .iter()
        .map(|row| children_of(document, *row)[ac.min(fc)..=ac.max(fc)].to_vec())
        .collect())
}

impl DocumentSession {
    pub(super) fn apply_table_intent(
        &mut self,
        intent: &EditIntent,
    ) -> Option<Result<SessionOutcome, SessionError>> {
        let action = match intent {
            EditIntent::Delete | EditIntent::Backspace
                if self.selection.active_cell_range().is_some() =>
            {
                self.plan_clear_cells()
            }
            EditIntent::InsertTableRow { table, index } => {
                self.plan_insert_table_row(*table, *index)
            }
            EditIntent::InsertTableColumn { table, index } => {
                self.plan_insert_table_column(*table, *index)
            }
            EditIntent::DeleteTableRow { table, index } => {
                self.plan_delete_table_row(*table, *index)
            }
            EditIntent::DeleteTableColumn { table, index } => {
                self.plan_delete_table_column(*table, *index)
            }
            _ => return None,
        };
        Some(action.and_then(|action| {
            self.history.break_group();
            self.clear_stored_marks();
            match action {
                PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
                PlannedAction::Commit(plan) => self.commit(plan),
                PlannedAction::CommitStaged(plan) => self.commit_staged(plan),
            }
        }))
    }

    fn plan_clear_cells(&self) -> Result<PlannedAction, SessionError> {
        let range = self
            .selection
            .active_cell_range()
            .ok_or(SessionError::SelectionInvalid)?;
        let mut transaction = user_transaction();
        for cell in range_cells(&self.document, range)?.into_iter().flatten() {
            let children = children_of(&self.document, cell);
            transaction.push_step(TransactionStep::InsertNode {
                parent: cell,
                index: 0,
                kind: NodeKind::Paragraph,
                attrs: NodeAttrs::empty(),
                content: NodeContent::empty_inline(),
            });
            for node in children {
                transaction.push_step(TransactionStep::RemoveNode { node });
            }
        }
        Ok(PlannedAction::Commit(
            EditPlan::new(transaction, SelectionUpdate::MapExisting, None)
                .with_history_policy(HistoryPolicy::Isolated),
        ))
    }
}
